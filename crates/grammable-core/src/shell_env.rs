// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Apps started from the Start menu or a desktop launcher often get a minimal PATH,
// so CLIs installed with npm, bun, volta, nvm or pipx are invisible. This builds the
// user's real PATH once and uses it to find and run agent CLIs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static CACHE: Mutex<Option<String>> = Mutex::new(None);

#[cfg(windows)]
const SEP: &str = ";";
#[cfg(not(windows))]
const SEP: &str = ":";

/// The resolved PATH used to find and run CLIs.
pub fn path() -> String {
    let mut cache = CACHE.lock().unwrap();
    if let Some(p) = cache.as_ref() {
        return p.clone();
    }
    let resolved = resolve();
    *cache = Some(resolved.clone());
    resolved
}

/// Forget the cached PATH (e.g. after the user installs a CLI).
pub fn reset() {
    *CACHE.lock().unwrap() = None;
}

/// Environment additions for every CLI run.
pub fn env_vars() -> Vec<(&'static str, String)> {
    vec![("PATH", path()), ("NO_COLOR", "1".into()), ("TERM", "dumb".into())]
}

/// Finds an executable by explicit path or by searching the resolved PATH.
pub fn find_executable(name: &str, custom: Option<&str>) -> Option<PathBuf> {
    if let Some(custom) = custom.map(str::trim).filter(|c| !c.is_empty()) {
        let expanded = expand_tilde(custom);
        return is_executable(&expanded).then_some(expanded);
    }
    for dir in std::env::split_paths(&OsString::from(path())) {
        for candidate in candidates(&dir, name) {
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// `true` for Windows batch files (npm's `.cmd` shims), which can't take
/// multi-line arguments safely; callers then send everything over stdin.
pub fn is_batch(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
}

fn expand_tilde(p: &str) -> PathBuf {
    match (p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(p),
    }
}

#[cfg(windows)]
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    // Prefer real executables over npm's .cmd shims.
    ["exe", "cmd", "bat", "com"].iter().map(|ext| dir.join(format!("{name}.{ext}"))).collect()
}

#[cfg(not(windows))]
fn candidates(dir: &Path, name: &str) -> Vec<PathBuf> {
    vec![dir.join(name)]
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

fn resolve() -> String {
    let home = dirs::home_dir().unwrap_or_default();
    let mut dirs: Vec<String> = Vec::new();

    #[cfg(unix)]
    if let Some(shell_path) = login_shell_path() {
        dirs.extend(shell_path.split(':').map(String::from));
    }
    if let Some(current) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&current).map(|p| p.to_string_lossy().into_owned()));
    }

    // Common install locations, in case the shell config is unusual.
    let h = |rel: &str| home.join(rel).to_string_lossy().into_owned();
    #[cfg(windows)]
    {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        dirs.extend([
            format!("{}\\npm", env("APPDATA")),
            h(".local\\bin"),
            h(".bun\\bin"),
            h(".cargo\\bin"),
            h("scoop\\shims"),
            format!("{}\\Programs\\Ollama", env("LOCALAPPDATA")),
            format!("{}\\Microsoft\\WinGet\\Links", env("LOCALAPPDATA")),
            format!("{}\\Volta\\bin", env("LOCALAPPDATA")),
            format!("{}\\nodejs", env("ProgramFiles")),
        ]);
    }
    #[cfg(not(windows))]
    {
        dirs.extend([
            h(".local/bin"),
            h(".claude/local"),
            h(".npm-global/bin"),
            h(".bun/bin"),
            h(".volta/bin"),
            h(".cargo/bin"),
            "/opt/homebrew/bin".into(),
            "/home/linuxbrew/.linuxbrew/bin".into(),
            "/usr/local/bin".into(),
            "/snap/bin".into(),
            "/usr/bin".into(),
            "/bin".into(),
        ]);
    }

    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| !d.is_empty() && seen.insert(d.clone()));
    dirs.join(SEP)
}

/// Asks the user's login shell for its PATH, giving up after a few seconds.
#[cfg(unix)]
fn login_shell_path() -> Option<String> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/bash".into());
    let marker = "__GRAMMABLE_PATH__";
    let mut child = Command::new(shell)
        .args(["-ilc", &format!("echo {marker}$PATH")])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_secs(4) => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|l| l.split_once(marker).map(|(_, p)| p.trim().to_string()))
        .filter(|p| !p.is_empty())
}
