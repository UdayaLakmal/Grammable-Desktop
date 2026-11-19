// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// App settings, stored as JSON in the user's config folder:
//   Linux:   ~/.config/Grammable/settings.json
//   Windows: %APPDATA%\Grammable\settings.json
// Shared by the command-line mode and (later) the desktop app.

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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            default_backend: BackendId::Claude, // same default as the macOS app
            fallback_to_built_in: true,
            backends: BackendConfig::default(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_file_keeps_defaults() {
        let s: Settings = serde_json::from_str(r#"{"defaultBackend":"ollama","backends":{"timeoutSecs":30}}"#).unwrap();
        assert_eq!(s.default_backend, BackendId::Ollama);
        assert!(s.fallback_to_built_in);
        assert_eq!(s.backends.timeout_secs, 30);
        assert_eq!(s.backends.ollama_url, "http://localhost:11434");
    }
}
