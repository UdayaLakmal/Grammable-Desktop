// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Every engine Grammable can drive. All of them are things the user already has
// (or can install for free); Grammable never ships its own model or server.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod builtin;
pub mod cli;
pub mod http;
pub mod runner;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BackendId {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "antigravity")]
    Antigravity,
    #[serde(rename = "devin")]
    Devin,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
    /// Offline spelling and grammar checker built into Grammable. No AI, fixes only.
    #[serde(rename = "builtin")]
    BuiltIn,
}

impl BackendId {
    pub const ALL: [BackendId; 8] = [
        BackendId::Claude,
        BackendId::Codex,
        BackendId::Gemini,
        BackendId::Antigravity,
        BackendId::Devin,
        BackendId::Ollama,
        BackendId::OpenAiCompatible,
        BackendId::BuiltIn,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            BackendId::Claude => "claude",
            BackendId::Codex => "codex",
            BackendId::Gemini => "gemini",
            BackendId::Antigravity => "antigravity",
            BackendId::Devin => "devin",
            BackendId::Ollama => "ollama",
            BackendId::OpenAiCompatible => "openai-compatible",
            BackendId::BuiltIn => "builtin",
        }
    }

    pub fn parse(s: &str) -> Option<BackendId> {
        let s = s.trim().to_lowercase();
        Self::ALL.into_iter().find(|b| b.as_str() == s)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            BackendId::Claude => "Claude Code",
            BackendId::Codex => "Codex CLI",
            BackendId::Gemini => "Gemini CLI",
            BackendId::Antigravity => "Antigravity CLI",
            BackendId::Devin => "Devin CLI",
            BackendId::Ollama => "Ollama",
            BackendId::OpenAiCompatible => "LM Studio / OpenAI-compatible",
            BackendId::BuiltIn => "Built-in Spelling & Grammar",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            BackendId::Claude => "Claude",
            BackendId::Codex => "Codex",
            BackendId::Gemini => "Gemini",
            BackendId::Antigravity => "Antigravity",
            BackendId::Devin => "Devin",
            BackendId::Ollama => "Ollama",
            BackendId::OpenAiCompatible => "Local server",
            BackendId::BuiltIn => "Built-in",
        }
    }

    /// `true` when the text never leaves this computer.
    pub fn is_on_device(self) -> bool {
        matches!(self, BackendId::Ollama | BackendId::OpenAiCompatible | BackendId::BuiltIn)
    }

    /// `false` for the built-in checker, which can fix spelling and grammar but not rewrite.
    pub fn can_rewrite(self) -> bool {
        self != BackendId::BuiltIn
    }

    pub fn executable_name(self) -> Option<&'static str> {
        match self {
            BackendId::Claude => Some("claude"),
            BackendId::Codex => Some("codex"),
            BackendId::Gemini => Some("gemini"),
            BackendId::Antigravity => Some("agy"),
            BackendId::Devin => Some("devin"),
            _ => None,
        }
    }

    pub fn privacy_note(self) -> &'static str {
        match self {
            BackendId::Claude => "Runs your installed Claude Code CLI. Text is sent to Anthropic under your own Claude account.",
            BackendId::Codex => "Runs your installed Codex CLI. Text is sent to OpenAI under your own ChatGPT / OpenAI account.",
            BackendId::Gemini => "Runs your installed Gemini CLI. Text is sent to Google under your own Google account.",
            BackendId::Antigravity => "Runs Google's Antigravity command-line tool (agy). Text is sent to Google under your own Antigravity account. Slower than the others.",
            BackendId::Devin => "Runs Cognition's Devin command-line tool. Text is sent to Cognition under your own Devin account.",
            BackendId::Ollama => "Runs a model on this computer through Ollama. Fully offline.",
            BackendId::OpenAiCompatible => "Talks to a local OpenAI-compatible server such as LM Studio, Jan or llama.cpp. Offline when the server is local.",
            BackendId::BuiltIn => "Grammable's own offline checker. Nothing to install, nothing leaves this computer. Fixes spelling and grammar only; rewrites and tone changes need an AI.",
        }
    }

    pub fn setup_hint(self) -> &'static str {
        match self {
            BackendId::Claude => "Install: npm install -g @anthropic-ai/claude-code — then run `claude` once to sign in.",
            BackendId::Codex => "Install: npm install -g @openai/codex — then run `codex` once to sign in.",
            BackendId::Gemini => "Install: npm install -g @google/gemini-cli — then run `gemini` once to sign in.",
            BackendId::Antigravity => "Install Antigravity's command-line tool (agy), then run `agy` once to sign in.",
            BackendId::Devin => "Install Devin's command-line tool, then run `devin auth login` once.",
            #[cfg(windows)]
            BackendId::Ollama => "Install: winget install Ollama.Ollama — then: ollama pull llama3.2",
            #[cfg(not(windows))]
            BackendId::Ollama => "Install: curl -fsSL https://ollama.com/install.sh | sh — then: ollama pull llama3.2",
            BackendId::OpenAiCompatible => "Start the local server in LM Studio (Developer → Start Server), Jan or llama.cpp.",
            BackendId::BuiltIn => "Built into Grammable. Nothing to install.",
        }
    }

    pub fn setup_url(self) -> &'static str {
        match self {
            BackendId::Claude => "https://docs.anthropic.com/en/docs/claude-code",
            BackendId::Codex => "https://github.com/openai/codex",
            BackendId::Gemini => "https://github.com/google-gemini/gemini-cli",
            BackendId::Antigravity => "https://antigravity.google/docs/cli/install",
            BackendId::Devin => "https://cognition.com/blog/devin-for-terminal",
            BackendId::Ollama => "https://ollama.com",
            BackendId::OpenAiCompatible => "https://lmstudio.ai",
            BackendId::BuiltIn => "https://github.com/UdayaLakmal/Grammable-Desktop#ai-backends",
        }
    }
}

/// User-tunable backend settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BackendConfig {
    /// Custom executable paths for CLI backends (empty = auto-detect).
    pub executable_paths: HashMap<BackendId, String>,
    /// Model per backend (empty = backend default).
    pub models: HashMap<BackendId, String>,
    pub ollama_url: String,
    pub openai_base_url: String,
    pub timeout_secs: u64,
}

impl Default for BackendConfig {
    fn default() -> Self {
        BackendConfig {
            executable_paths: HashMap::new(),
            models: HashMap::new(),
            ollama_url: "http://localhost:11434".into(),
            openai_base_url: "http://localhost:1234/v1".into(),
            timeout_secs: 90,
        }
    }
}

impl BackendConfig {
    pub fn model(&self, id: BackendId, override_model: Option<&str>) -> Option<String> {
        if let Some(m) = override_model.filter(|m| !m.trim().is_empty()) {
            return Some(m.trim().to_string());
        }
        self.models.get(&id).map(|m| m.trim().to_string()).filter(|m| !m.is_empty())
    }

    pub fn executable_path(&self, id: BackendId) -> Option<&str> {
        self.executable_paths.get(&id).map(String::as_str)
    }

    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout_secs.max(5))
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BackendError {
    #[error("{} was not found on this computer. {}", .0.display_name(), .0.setup_hint())]
    NotInstalled(BackendId),
    #[error("Could not reach {}. {}", .0.display_name(), .1)]
    NotReachable(BackendId, String),
    #[error("{0}")]
    Unavailable(String),
    #[error("The AI did not answer within {0} seconds. Try again or pick a faster backend.")]
    TimedOut(u64),
    #[error("{0}")]
    Failed(String),
    #[error("The AI returned an empty answer. Try again.")]
    EmptyResponse,
    #[error("Cancelled.")]
    Cancelled,
}

impl BackendError {
    /// The AI isn't installed, running or usable (as opposed to failing mid-request).
    pub fn means_not_set_up(&self) -> bool {
        matches!(self, BackendError::NotInstalled(_) | BackendError::NotReachable(..) | BackendError::Unavailable(_))
    }
}

/// A single request: instruction + text.
#[derive(Debug, Clone)]
pub struct AiRequest {
    pub instructions: String,
    pub text: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackendStatus {
    pub available: bool,
    /// Path, URL or version info for display.
    pub detail: String,
}

impl BackendStatus {
    pub fn available(detail: impl Into<String>) -> Self {
        BackendStatus { available: true, detail: detail.into() }
    }
    pub fn missing(detail: impl Into<String>) -> Self {
        BackendStatus { available: false, detail: detail.into() }
    }
}

/// Returns the raw model output (not yet cleaned).
pub async fn complete(id: BackendId, config: &BackendConfig, request: &AiRequest) -> Result<String, BackendError> {
    match id {
        BackendId::Claude => cli::claude(config, request).await,
        BackendId::Codex => cli::codex(config, request).await,
        BackendId::Gemini => cli::gemini(config, request).await,
        BackendId::Antigravity => cli::antigravity(config, request).await,
        BackendId::Devin => cli::devin(config, request).await,
        BackendId::Ollama => http::ollama(config, request).await,
        BackendId::OpenAiCompatible => http::openai_compatible(config, request).await,
        BackendId::BuiltIn => builtin::check(&request.text),
    }
}

/// Fast check used to show status in the UI. Never prompts the user.
pub async fn check_availability(id: BackendId, config: &BackendConfig) -> BackendStatus {
    match id {
        BackendId::Devin => cli::devin_status(config).await,
        BackendId::Ollama => http::ollama_status(config).await,
        BackendId::OpenAiCompatible => http::openai_status(config).await,
        BackendId::BuiltIn => BackendStatus::available("Built in, offline"),
        cli_id => match cli::executable(cli_id, config) {
            Some(path) => BackendStatus::available(path.to_string_lossy()),
            None => BackendStatus::missing(format!("Not found. {}", cli_id.setup_hint())),
        },
    }
}

/// First-run choice of default backend: the first AI that is ready, else the built-in checker.
/// Same order as the macOS app (minus Apple Intelligence).
pub async fn first_available(config: &BackendConfig) -> BackendId {
    use BackendId::*;
    // Checked concurrently; the order only decides which ready backend wins.
    let (claude, codex, gemini, ollama, openai) = tokio::join!(
        check_availability(Claude, config),
        check_availability(Codex, config),
        check_availability(Gemini, config),
        check_availability(Ollama, config),
        check_availability(OpenAiCompatible, config),
    );
    [(Claude, claude), (Codex, codex), (Gemini, gemini), (Ollama, ollama), (OpenAiCompatible, openai)]
        .into_iter()
        .find(|(_, status)| status.available)
        .map(|(id, _)| id)
        .unwrap_or(BuiltIn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_available_falls_back_to_built_in() {
        let mut c = BackendConfig {
            ollama_url: "http://127.0.0.1:9".into(),
            openai_base_url: "http://127.0.0.1:9/v1".into(),
            ..Default::default()
        };
        for id in [BackendId::Claude, BackendId::Codex, BackendId::Gemini] {
            c.executable_paths.insert(id, "/nonexistent/grammable-test".into());
        }
        assert_eq!(first_available(&c).await, BackendId::BuiltIn);
    }

    #[test]
    fn ids_round_trip() {
        for id in BackendId::ALL {
            assert_eq!(BackendId::parse(id.as_str()), Some(id));
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, format!("\"{}\"", id.as_str()));
        }
        assert_eq!(BackendId::parse(" Ollama "), Some(BackendId::Ollama));
        assert_eq!(BackendId::parse("apple"), None);
    }

    #[test]
    fn model_override_wins() {
        let mut c = BackendConfig::default();
        c.models.insert(BackendId::Ollama, " llama3.2 ".into());
        assert_eq!(c.model(BackendId::Ollama, None).as_deref(), Some("llama3.2"));
        assert_eq!(c.model(BackendId::Ollama, Some("qwen")).as_deref(), Some("qwen"));
        assert_eq!(c.model(BackendId::Claude, Some("  ")), None);
    }

    #[test]
    fn config_deserializes_with_missing_fields() {
        let c: BackendConfig = serde_json::from_str(r#"{"models":{"ollama":"phi3"}}"#).unwrap();
        assert_eq!(c.ollama_url, "http://localhost:11434");
        assert_eq!(c.models.get(&BackendId::Ollama).map(String::as_str), Some("phi3"));
    }
}
