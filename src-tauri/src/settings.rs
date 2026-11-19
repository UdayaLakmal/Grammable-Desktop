// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// App settings, stored as JSON in the user's config folder:
//   Linux:   ~/.config/Grammable/settings.json
//   Windows: %APPDATA%\Grammable\settings.json
// Shared by the command-line mode and the desktop app.

use std::path::PathBuf;

use grammable_core::{BackendConfig, BackendId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub default_backend: BackendId,
    /// Grammar fixes use the built-in checker when the chosen AI isn't installed or running.
    pub fallback_to_built_in: bool,
    pub backends: BackendConfig,

    /// Master switch: hotkey and G button do nothing while off.
    pub is_enabled: bool,
    /// Show the floating "G" button after selecting text (Windows and X11).
    pub popup_on_selection: bool,
    /// Selections shorter than this (trimmed, in characters) are ignored.
    pub min_selection_length: usize,
    /// Global shortcut in Tauri accelerator syntax, e.g. "Ctrl+Alt+G".
    pub hotkey: String,
    /// Process names (without `.exe`, case-insensitive) Grammable never reads from.
    pub excluded_apps: Vec<String>,
    pub has_completed_onboarding: bool,
}

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+G";

/// Terminals and password managers: never read or paste there.
pub fn default_excluded_apps() -> Vec<String> {
    let apps: &[&str] = if cfg!(windows) {
        &["WindowsTerminal", "cmd", "powershell", "pwsh", "1Password", "Bitwarden", "KeePassXC", "KeePass"]
    } else {
        &[
            "gnome-terminal-server", "ptyxis", "konsole", "xfce4-terminal", "tilix", "kitty", "alacritty",
            "wezterm-gui", "ghostty", "xterm", "1password", "bitwarden", "keepassxc",
        ]
    };
    apps.iter().map(|s| s.to_string()).collect()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            default_backend: BackendId::Claude, // same default as the macOS app
            fallback_to_built_in: true,
            backends: BackendConfig::default(),
            is_enabled: true,
            popup_on_selection: true,
            min_selection_length: 3,
            hotkey: DEFAULT_HOTKEY.into(),
            excluded_apps: default_excluded_apps(),
            has_completed_onboarding: false,
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Grammable")
            .join("settings.json")
    }

    /// `false` until the settings have been saved once (first launch).
    pub fn exists() -> bool {
        Self::path().exists()
    }

    /// Missing file → defaults. Unreadable or invalid file → defaults plus a warning.
    pub fn load() -> (Settings, Option<String>) {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Settings::default(), None),
            Err(e) => (Settings::default(), Some(format!("Could not read {}: {e}", path.display()))),
            Ok(text) => match serde_json::from_str(&text) {
                Ok(s) => (s, None),
                Err(e) => (Settings::default(), Some(format!("Ignoring invalid {}: {e}", path.display()))),
            },
        }
    }

    /// Writes atomically (temp file + rename) so a crash never leaves half a file.
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json + "\n")?;
        std::fs::rename(&tmp, &path)
    }

    /// Is `process` (a name like "kitty" or "WindowsTerminal.exe") excluded?
    pub fn is_excluded(&self, process: &str) -> bool {
        let name = process.trim();
        let name = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(name);
        self.excluded_apps.iter().any(|e| e.trim().eq_ignore_ascii_case(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_keeps_defaults() {
        let s: Settings = serde_json::from_str(r#"{"defaultBackend":"ollama","backends":{"timeoutSecs":30}}"#).unwrap();
        assert_eq!(s.default_backend, BackendId::Ollama);
        assert!(s.fallback_to_built_in);
        assert!(s.is_enabled);
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert_eq!(s.min_selection_length, 3);
        assert_eq!(s.backends.timeout_secs, 30);
        assert_eq!(s.backends.ollama_url, "http://localhost:11434");
    }

    #[test]
    fn round_trips_through_json() {
        let mut s = Settings::default();
        s.default_backend = BackendId::Gemini;
        s.backends.models.insert(BackendId::Ollama, "llama3.2".into());
        s.excluded_apps = vec!["Foo".into()];
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn exclusion_ignores_case_and_exe() {
        let s = Settings { excluded_apps: vec!["WindowsTerminal".into(), "kitty".into()], ..Default::default() };
        assert!(s.is_excluded("windowsterminal.exe"));
        assert!(s.is_excluded("Kitty"));
        assert!(!s.is_excluded("firefox"));
    }
}
