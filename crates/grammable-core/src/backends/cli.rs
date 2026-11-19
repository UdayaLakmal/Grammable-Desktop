// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Agent CLIs that are already installed and signed in on this computer.
// Grammable only *runs* them; it never stores keys or tokens.
//
// Windows note: npm installs CLIs as `.cmd` batch shims, which can't take
// multi-line arguments safely. For those, everything goes over stdin.

use std::path::PathBuf;

use serde_json::Value;

use super::runner::{self, LineDecision};
use super::{AiRequest, BackendConfig, BackendError, BackendId, BackendStatus};
use crate::prompt::{combined_prompt, user_prompt, SYSTEM_PROMPT};
use crate::shell_env;

pub fn executable(id: BackendId, config: &BackendConfig) -> Option<PathBuf> {
    shell_env::find_executable(id.executable_name()?, config.executable_path(id))
}

fn require(id: BackendId, config: &BackendConfig) -> Result<PathBuf, BackendError> {
    executable(id, config).ok_or(BackendError::NotInstalled(id))
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// `claude -p` with tools, MCP servers, settings and session history disabled:
/// a plain, fast, single-turn text completion on the user's own Claude plan.
pub async fn claude(config: &BackendConfig, request: &AiRequest) -> Result<String, BackendError> {
    let id = BackendId::Claude;
    let exe = require(id, config)?;
    let batch = shell_env::is_batch(&exe);

    let mut args = s(&["-p", "--output-format", "text"]);
    if !batch {
        args.extend(s(&["--system-prompt", SYSTEM_PROMPT]));
    }
    args.extend(s(&["--tools", "", "--strict-mcp-config", "--setting-sources", "", "--no-session-persistence"]));
    if let Some(model) = config.model(id, request.model.as_deref()) {
        args.extend(["--model".into(), model]);
    }
    let input = if batch {
        combined_prompt(&request.instructions, &request.text)
    } else {
        user_prompt(&request.instructions, &request.text)
    };

    let result = runner::run(&exe, &args, Some(input), config.timeout()).await?;
    if result.exit_code != 0 {
        return Err(runner::failure(&result, "claude"));
    }
    Ok(result.stdout)
}

/// `codex exec`; the final answer is read from `--output-last-message`.
pub async fn codex(config: &BackendConfig, request: &AiRequest) -> Result<String, BackendError> {
    let id = BackendId::Codex;
    let exe = require(id, config)?;
    let out_file = runner::temp_file("grammable-codex");

    let mut args = s(&["exec", "--skip-git-repo-check", "--color", "never", "--output-last-message"]);
    args.push(out_file.to_string_lossy().into_owned());
    if let Some(model) = config.model(id, request.model.as_deref()) {
        args.extend(["--model".into(), model]);
    }
    args.push("-".into()); // read the prompt from stdin

    let result = runner::run(&exe, &args, Some(combined_prompt(&request.instructions, &request.text)), config.timeout()).await;
    let last = std::fs::read_to_string(&out_file).ok();
    let _ = std::fs::remove_file(&out_file);
    let result = result?;
    if result.exit_code != 0 {
        return Err(runner::failure(&result, "codex"));
    }
    match last {
        Some(text) if !text.trim().is_empty() => Ok(text),
        _ => Ok(result.stdout),
    }
}

/// Gemini CLI in non-interactive mode on the user's own Google account.
pub async fn gemini(config: &BackendConfig, request: &AiRequest) -> Result<String, BackendError> {
    let id = BackendId::Gemini;
    let exe = require(id, config)?;
    let prompt = combined_prompt(&request.instructions, &request.text);

    let mut args = Vec::new();
    if let Some(model) = config.model(id, request.model.as_deref()) {
        args.extend(["--model".into(), model]);
    }
    // Piped stdin puts Gemini CLI in non-interactive mode, which avoids long arguments through `.cmd` shims.
    let stdin = if shell_env::is_batch(&exe) {
        Some(prompt)
    } else {
        args.extend(["--prompt".into(), prompt]);
        None
    };

    let result = runner::run(&exe, &args, stdin, config.timeout()).await?;
    if result.exit_code != 0 {
        return Err(runner::failure(&result, "gemini"));
    }
    Ok(result.stdout)
}

/// Google Antigravity's command-line tool, `agy -p`, streaming JSON events.
pub async fn antigravity(config: &BackendConfig, request: &AiRequest) -> Result<String, BackendError> {
    let id = BackendId::Antigravity;
    let exe = require(id, config)?;
    let mut args = vec![
        "-p".into(),
        combined_prompt(&request.instructions, &request.text),
        "--output-format".into(),
        "stream-json".into(),
        "--print-timeout".into(),
        format!("{}s", config.timeout_secs.saturating_sub(5).max(10)),
        "--disable-slash-commands".into(),
    ];
    if let Some(model) = config.model(id, request.model.as_deref()) {
        args.extend(["--model".into(), model]);
    }

    let mut parser = AntigravityStreamParser::default();
    let answer = runner::run_streaming(&exe, &args, config.timeout(), |line| parser.handle(line))
        .await
        .map_err(|e| match e {
            BackendError::Failed(m) if m.to_lowercase().contains("authentication required") => not_signed_in_agy(),
            other => other,
        })?;
    if answer.trim().is_empty() {
        return Err(BackendError::EmptyResponse);
    }
    Ok(answer)
}

fn not_signed_in_agy() -> BackendError {
    BackendError::Unavailable("Antigravity CLI isn't signed in. Run `agy` once in a terminal to sign in.".into())
}

/// Reads `agy --output-format stream-json` events.
#[derive(Default)]
pub(crate) struct AntigravityStreamParser {
    text: String,
}

impl AntigravityStreamParser {
    pub(crate) fn handle(&mut self, line: &str) -> LineDecision {
        let Ok(event) = serde_json::from_str::<Value>(line) else { return LineDecision::KeepReading };
        let str_of = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(String::from);

        match event.get("event").and_then(Value::as_str) {
            Some("step_update") => {
                let Some(step) = event.get("step_update") else { return LineDecision::KeepReading };
                let kind = str_of(step, "step_type").unwrap_or_default();
                let state = str_of(step, "state").unwrap_or_default();
                if kind == "agent_response" {
                    self.text.push_str(&str_of(step, "text_delta").unwrap_or_default());
                    if state == "DONE" {
                        // A response step can finish with no text (thinking only); wait for the real answer.
                        if self.text.trim().is_empty() {
                            self.text.clear();
                            return LineDecision::KeepReading;
                        }
                        return LineDecision::Finished(std::mem::take(&mut self.text));
                    }
                } else if kind == "error_message" {
                    let message = str_of(step, "text").or_else(|| str_of(step, "text_delta")).or_else(|| str_of(step, "error"));
                    let detail = message.map(|m| format!(": {m}")).unwrap_or_default();
                    return LineDecision::Failed(BackendError::Failed(format!(
                        "Antigravity reported an error{detail}. Try again, or pick another backend."
                    )));
                }
            }
            Some("result") => {
                let result = event.get("result").cloned().unwrap_or(Value::Null);
                if let Some(r) = str_of(&result, "response").filter(|r| !r.is_empty()) {
                    return LineDecision::Finished(r);
                }
                if !self.text.is_empty() {
                    return LineDecision::Finished(std::mem::take(&mut self.text));
                }
                let status = str_of(&result, "status").unwrap_or_else(|| "unknown".into());
                return LineDecision::Failed(BackendError::Failed(format!(
                    "Antigravity finished without an answer (status {status})."
                )));
            }
            Some("error") => {
                let err = event.get("error").cloned().unwrap_or(Value::Null);
                let message = str_of(&err, "message").or_else(|| err.as_str().map(String::from)).unwrap_or_else(|| "unknown error".into());
                if message.to_lowercase().contains("auth") {
                    return LineDecision::Failed(not_signed_in_agy());
                }
                return LineDecision::Failed(BackendError::Failed(format!("Antigravity: {message}")));
            }
            _ => {}
        }
        LineDecision::KeepReading
    }
}

pub async fn devin_status(config: &BackendConfig) -> BackendStatus {
    let Some(path) = executable(BackendId::Devin, config) else {
        return BackendStatus::missing(format!("Not found. {}", BackendId::Devin.setup_hint()));
    };
    if let Ok(status) = runner::run(&path, &s(&["auth", "status"]), None, std::time::Duration::from_secs(8)).await {
        if format!("{}{}", status.stdout, status.stderr).contains("Not logged in") {
            return BackendStatus::missing(format!(
                "Installed at {}, but not signed in. Run `devin auth login` once in a terminal.",
                path.display()
            ));
        }
    }
    BackendStatus::available(path.to_string_lossy())
}

/// Devin CLI in print mode, read-only tools, in Grammable's empty working folder.
pub async fn devin(config: &BackendConfig, request: &AiRequest) -> Result<String, BackendError> {
    let id = BackendId::Devin;
    let exe = require(id, config)?;
    let prompt_file = runner::temp_file("grammable-devin");
    std::fs::write(&prompt_file, combined_prompt(&request.instructions, &request.text))
        .map_err(|e| BackendError::Failed(format!("Could not write prompt file: {e}")))?;

    let mut args = vec!["-p".to_string(), "--prompt-file".into(), prompt_file.to_string_lossy().into_owned()];
    args.extend(s(&[
        "--permission-mode", "auto",
        "--respect-workspace-trust", "false",
        "--process-model", "in-process",
        "--verbosity", "0",
    ]));
    if let Some(model) = config.model(id, request.model.as_deref()) {
        args.extend(["--model".into(), model]);
    }

    let result = runner::run(&exe, &args, None, config.timeout()).await;
    let _ = std::fs::remove_file(&prompt_file);
    let result = result?;
    if result.exit_code != 0 {
        let message = format!("{}{}", result.stderr, result.stdout).to_lowercase();
        if message.contains("login") || message.contains("not logged in") {
            return Err(BackendError::Unavailable(
                "Devin CLI isn't signed in. Run `devin auth login` once in a terminal.".into(),
            ));
        }
        return Err(runner::failure(&result, "devin"));
    }
    Ok(runner::strip_terminal_codes(&result.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_parser_collects_response() {
        let mut p = AntigravityStreamParser::default();
        let step = |delta: &str, state: &str| {
            format!(r#"{{"event":"step_update","step_update":{{"step_type":"agent_response","state":"{state}","text_delta":"{delta}"}}}}"#)
        };
        assert!(matches!(p.handle("not json"), LineDecision::KeepReading));
        assert!(matches!(p.handle(&step("", "DONE")), LineDecision::KeepReading));
        assert!(matches!(p.handle(&step("Hello ", "RUNNING")), LineDecision::KeepReading));
        match p.handle(&step("world", "DONE")) {
            LineDecision::Finished(t) => assert_eq!(t, "Hello world"),
            _ => panic!("expected an answer"),
        }
    }

    #[test]
    fn antigravity_parser_reports_auth_errors() {
        let mut p = AntigravityStreamParser::default();
        match p.handle(r#"{"event":"error","error":{"message":"auth required"}}"#) {
            LineDecision::Failed(BackendError::Unavailable(m)) => assert!(m.contains("sign in")),
            _ => panic!("expected sign-in error"),
        }
    }
}
