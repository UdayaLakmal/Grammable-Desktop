// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Runs command-line programs with a timeout. Dropping the returned future
// (e.g. the user pressed Cancel) kills the process.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use regex::Regex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::BackendError;
use crate::shell_env;

#[derive(Debug, Clone)]
pub struct CliResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// An empty, private working directory so agent CLIs never pick up project files
/// such as CLAUDE.md, AGENTS.md or GEMINI.md from wherever the app was launched.
pub fn working_directory() -> PathBuf {
    let dir = std::env::temp_dir().join("grammable-work");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// A unique temporary file path.
pub fn temp_file(prefix: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("{prefix}-{}-{nanos}.txt", std::process::id()))
}

/// Removes terminal control sequences that some CLIs print even into a pipe.
pub fn strip_terminal_codes(text: &str) -> String {
    static CSI: OnceLock<Regex> = OnceLock::new();
    static OSC: OnceLock<Regex> = OnceLock::new();
    let csi = CSI.get_or_init(|| Regex::new("\x1B\\[[0-9;?]*[A-Za-z]").unwrap());
    let osc = OSC.get_or_init(|| Regex::new("\x1B\\][^\x07]*\x07").unwrap());
    osc.replace_all(&csi.replace_all(text, ""), "").into_owned()
}

fn command(executable: &Path, arguments: &[String]) -> Command {
    let mut cmd = Command::new(executable);
    cmd.args(arguments)
        .envs(shell_env::env_vars())
        .current_dir(working_directory())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000; // never flash a console window
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn start_error(executable: &Path, e: std::io::Error) -> BackendError {
    BackendError::Failed(format!("Could not start {}: {e}", executable.display()))
}

pub async fn run(
    executable: &Path,
    arguments: &[String],
    stdin: Option<String>,
    timeout: Duration,
) -> Result<CliResult, BackendError> {
    let mut cmd = command(executable, arguments);
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| start_error(executable, e))?;

    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        tokio::spawn(async move {
            let _ = pipe.write_all(input.as_bytes()).await;
            let _ = pipe.shutdown().await;
        });
    }

    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(out)) => Ok(CliResult {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            exit_code: out.status.code().unwrap_or(-1),
        }),
        Ok(Err(e)) => Err(BackendError::Failed(format!("{} failed: {e}", executable.display()))),
        // The child is dropped with the future and killed (kill_on_drop).
        Err(_) => Err(BackendError::TimedOut(timeout.as_secs())),
    }
}

/// What a line handler wants after reading one line of streamed output.
pub enum LineDecision {
    KeepReading,
    Finished(String),
    Failed(BackendError),
}

/// Runs a CLI that streams one JSON event per line and returns as soon as `handle` has the answer.
///
/// Once the answer is in, the process is left to exit on its own while its output is drained in
/// the background: some CLIs (Antigravity) stall their next run when they are killed mid-turn.
pub async fn run_streaming(
    executable: &Path,
    arguments: &[String],
    timeout: Duration,
    mut handle: impl FnMut(&str) -> LineDecision,
) -> Result<String, BackendError> {
    let mut cmd = command(executable, arguments);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| start_error(executable, e))?;

    let stderr_tail = Arc::new(Mutex::new(String::new()));
    if let Some(err) = child.stderr.take() {
        let tail = stderr_tail.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut t = tail.lock().unwrap();
                t.push_str(&line);
                t.push('\n');
                if t.len() > 8192 {
                    let cut = t.len() - 4096;
                    let cut = (cut..t.len()).find(|i| t.is_char_boundary(*i)).unwrap_or(0);
                    *t = t[cut..].to_string();
                }
            }
        });
    }

    let stdout = child.stdout.take().expect("stdout is piped");
    let mut lines = BufReader::new(stdout).lines();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match tokio::time::timeout_at(deadline, lines.next_line()).await {
            Err(_) => return Err(BackendError::TimedOut(timeout.as_secs())),
            Ok(Err(e)) => return Err(BackendError::Failed(format!("Reading output failed: {e}"))),
            Ok(Ok(None)) => {
                let status = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(-1);
                let tail = last_lines(&stderr_tail.lock().unwrap(), 3);
                let name = executable.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let detail = if tail.is_empty() { String::new() } else { format!("\n{tail}") };
                return Err(BackendError::Failed(format!("{name} exited with code {status} before answering.{detail}")));
            }
            Ok(Ok(Some(line))) => match handle(&line) {
                LineDecision::KeepReading => {}
                LineDecision::Failed(e) => return Err(e),
                LineDecision::Finished(answer) => {
                    // Let the CLI wrap up by itself; keep draining so it never blocks on a full pipe.
                    tokio::spawn(async move {
                        while let Ok(Some(_)) = lines.next_line().await {}
                        let _ = child.wait().await;
                    });
                    return Ok(answer);
                }
            },
        }
    }
}

fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Turns a non-zero exit into a readable error, keeping the most useful lines.
pub fn failure(result: &CliResult, tool: &str) -> BackendError {
    let source = if result.stderr.trim().is_empty() { &result.stdout } else { &result.stderr };
    let tail = last_lines(&strip_terminal_codes(source), 4);
    let lower = tail.to_lowercase();
    let hint = if lower.contains("login") || lower.contains("auth") {
        format!("\nOpen a terminal and run `{tool}` once to sign in.")
    } else {
        String::new()
    };
    BackendError::Failed(format!("{tool} exited with code {}.\n{tail}{hint}", result.exit_code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_codes() {
        assert_eq!(strip_terminal_codes("\x1B[31mred\x1B[0m \x1B]0;title\x07ok"), "red ok");
    }

    #[test]
    fn failure_mentions_sign_in() {
        let r = CliResult { stdout: String::new(), stderr: "x\nPlease login first\n".into(), exit_code: 1 };
        let BackendError::Failed(msg) = failure(&r, "claude") else { panic!() };
        assert!(msg.contains("code 1"));
        assert!(msg.contains("run `claude` once"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_with_stdin_and_times_out() {
        let r = run(Path::new("/bin/cat"), &[], Some("hello".into()), Duration::from_secs(5)).await.unwrap();
        assert_eq!(r.stdout, "hello");
        assert_eq!(r.exit_code, 0);
        let slow = run(Path::new("/bin/sleep"), &["5".into()], None, Duration::from_millis(200)).await;
        assert!(matches!(slow, Err(BackendError::TimedOut(_))));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn streaming_returns_first_answer() {
        let args = vec!["-c".to_string(), "echo one; echo answer; sleep 2".to_string()];
        let out = run_streaming(Path::new("/bin/sh"), &args, Duration::from_secs(5), |line| {
            if line == "answer" { LineDecision::Finished(line.into()) } else { LineDecision::KeepReading }
        })
        .await
        .unwrap();
        assert_eq!(out, "answer");
    }
}
