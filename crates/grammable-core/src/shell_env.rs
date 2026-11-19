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

/// Editors whose extension folders are searched for bundled CLIs (under the home folder).
const EDITORS: [&str; 5] = [".vscode", ".vscode-insiders", ".vscode-oss", ".cursor", ".windsurf"];

/// A copy of an agent CLI that ships inside an app or editor extension, for people who
/// use the tool only through that app and never put it on PATH. Returns the newest
/// version found, or `None` for CLIs that aren't bundled anywhere.
pub fn bundled(name: &str) -> Option<PathBuf> {
    newest_bundled(name, dirs::config_dir().as_deref(), dirs::home_dir().as_deref())
}

/// Searches, under the given config and home folders:
///   claude: <config>/Claude/claude-code/<version>/<build>/claude          (Claude desktop app)
///           <home>/<editor>/extensions/anthropic.claude-code-<version>-<platform>/resources/native-binary/claude
///   codex:  <home>/<editor>/extensions/openai.chatgpt-<version>-<platform>/bin/<platform>/codex
/// (`.exe` on Windows.) Gemini, Antigravity and Devin CLIs are standalone installs only.
pub(crate) fn newest_bundled(name: &str, config: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut found: Vec<(Vec<u64>, PathBuf)> = Vec::new();
    let mut consider = |version: Vec<u64>, candidate: PathBuf| {
        if is_executable(&candidate) {
            found.push((version, candidate));
        }
    };
    let extensions = |prefix: &'static str| {
        home.into_iter()
            .flat_map(|home| EDITORS.iter().flat_map(move |e| read_dirs(&home.join(e).join("extensions"))))
            .filter_map(move |dir| version_of(&dir, prefix).map(|v| (v, dir)))
    };

    match name {
        "claude" => {
            if let Some(config) = config {
                for version_dir in read_dirs(&config.join("Claude").join("claude-code")) {
                    let Some(version) = version_of(&version_dir, "") else { continue };
                    for build_dir in read_dirs(&version_dir) {
                        consider(version.clone(), build_dir.join(&exe));
                    }
                }
            }
            for (version, dir) in extensions("anthropic.claude-code-") {
                consider(version, dir.join("resources").join("native-binary").join(&exe));
            }
        }
        "codex" => {
            for (version, dir) in extensions("openai.chatgpt-") {
                // One platform folder per build, e.g. bin/linux-x86_64 or bin/windows-x86_64.
                for platform_dir in read_dirs(&dir.join("bin")) {
                    consider(version.clone(), platform_dir.join(&exe));
                }
            }
        }
        _ => {}
    }
    found.into_iter().max_by(|a, b| a.0.cmp(&b.0)).map(|(_, path)| path)
}

fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

/// "2.1.328" from a folder named `<prefix>2.1.328[-linux-x64]`, as comparable numbers.
fn version_of(dir: &Path, prefix: &str) -> Option<Vec<u64>> {
    let name = dir.file_name()?.to_str()?.strip_prefix(prefix)?;
    let version = name.split('-').next()?;
    let parts: Option<Vec<u64>> = version.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
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
            format!("{}\\agy\\bin", env("LOCALAPPDATA")),   // Antigravity CLI installer
            format!("{}\\devin\\cli", env("LOCALAPPDATA")), // Devin CLI installer
            format!("{}\\nodejs", env("ProgramFiles")),
        ]);
    }
    #[cfg(not(windows))]
    {
        dirs.extend([
            h(".local/bin"),
            h(".claude/local"),
            h(".codeium/windsurf/bin"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_exe(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn finds_newest_bundled_claude() {
        let root = std::env::temp_dir().join(format!("grammable-bundled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (config, home) = (root.join("config"), root.join("home"));
        let exe = if cfg!(windows) { "claude.exe" } else { "claude" };
        assert_eq!(newest_bundled("claude", Some(&config), Some(&home)), None);

        make_exe(&config.join("Claude/claude-code/2.1.317/cd557f900744").join(exe));
        make_exe(&home.join(".vscode/extensions/anthropic.claude-code-2.1.9-linux-x64/resources/native-binary").join(exe));
        // Not a Claude extension, and a folder without the binary: both ignored.
        make_exe(&home.join(".vscode/extensions/other.tool-9.9.9/resources/native-binary").join(exe));
        std::fs::create_dir_all(home.join(".cursor/extensions/anthropic.claude-code-3.0.0-linux-x64")).unwrap();
        let found = newest_bundled("claude", Some(&config), Some(&home)).unwrap();
        assert!(found.to_string_lossy().contains("2.1.317"), "{found:?}");

        // 2.1.328 beats 2.1.317 numerically (not as text).
        make_exe(&home.join(".vscode/extensions/anthropic.claude-code-2.1.328-linux-x64/resources/native-binary").join(exe));
        let found = newest_bundled("claude", Some(&config), Some(&home)).unwrap();
        assert!(found.to_string_lossy().contains("2.1.328"), "{found:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn finds_codex_in_the_openai_extension() {
        let root = std::env::temp_dir().join(format!("grammable-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let exe = if cfg!(windows) { "codex.exe" } else { "codex" };
        assert_eq!(newest_bundled("codex", None, Some(&home)), None);
        make_exe(&home.join(".vscode/extensions/openai.chatgpt-26.1021.22027-linux-x64/bin/linux-x86_64").join(exe));
        make_exe(&home.join(".cursor/extensions/openai.chatgpt-26.903.1-linux-x64/bin/linux-x86_64").join(exe));
        let found = newest_bundled("codex", None, Some(&home)).unwrap();
        assert!(found.to_string_lossy().contains("26.1021.22027"), "{found:?}");
        assert_eq!(newest_bundled("gemini", None, Some(&home)), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parses_versions() {
        assert_eq!(version_of(Path::new("/x/2.1.317"), ""), Some(vec![2, 1, 317]));
        assert_eq!(version_of(Path::new("/x/anthropic.claude-code-2.1.328-linux-x64"), "anthropic.claude-code-"), Some(vec![2, 1, 328]));
        assert_eq!(version_of(Path::new("/x/latest"), ""), None);
    }
}
