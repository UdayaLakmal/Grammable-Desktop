// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Runs a preset against text with the right backend and post-processing.

use std::time::Instant;

use serde::Serialize;

use crate::backends::{self, AiRequest, BackendConfig, BackendError, BackendId};
use crate::diff::TextDiff;
use crate::presets::{Preset, PresetMode};
use crate::prompt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineResult {
    pub preset_id: String,
    pub preset_name: String,
    pub mode: PresetMode,
    pub backend: BackendId,
    pub backend_name: String,
    pub on_device: bool,
    pub original: String,
    pub suggestion: String,
    pub diff: TextDiff,
    pub duration_ms: u128,
    pub unchanged: bool,
}

pub const NEEDS_AI_MESSAGE: &str = "The built-in checker can fix spelling and grammar, but rewrites and tone changes need an AI. \
Set up a free offline AI (Ollama) in Settings › AI Backends, or use Claude Code, Codex or Gemini CLI.";

/// The backend a preset will use: its own override, otherwise the default.
pub fn resolve_backend(preset: &Preset, default: BackendId) -> BackendId {
    preset.backend_id().unwrap_or(default)
}

pub async fn run(
    preset: &Preset,
    text: &str,
    default_backend: BackendId,
    config: &BackendConfig,
    fallback_to_built_in: bool,
) -> Result<EngineResult, BackendError> {
    let backend = resolve_backend(preset, default_backend);
    let start = Instant::now();

    if !backend.can_rewrite() && preset.mode != PresetMode::Fix {
        return Err(BackendError::Unavailable(NEEDS_AI_MESSAGE.into()));
    }

    let request = AiRequest {
        instructions: preset.instructions.clone(),
        text: text.to_string(),
        model: if preset.backend_id() == Some(backend) { preset.model.clone() } else { None },
    };

    let mut used = backend;
    let raw = match backends::complete(backend, config, &request).await {
        Ok(raw) => raw,
        // No AI installed or running: grammar and spelling fixes still work with the built-in checker.
        Err(e) if fallback_to_built_in && preset.mode == PresetMode::Fix && e.means_not_set_up() => {
            used = BackendId::BuiltIn;
            backends::complete(BackendId::BuiltIn, config, &request).await?
        }
        Err(e) => return Err(e),
    };

    // The built-in checker returns the corrected text itself, with no model chatter to strip.
    let suggestion = if used == BackendId::BuiltIn { raw } else { prompt::clean(&raw, text) };
    if suggestion.trim().is_empty() {
        return Err(BackendError::EmptyResponse);
    }
    let diff = TextDiff::new(text, &suggestion);
    Ok(EngineResult {
        preset_id: preset.id.clone(),
        preset_name: preset.name.clone(),
        mode: preset.mode,
        backend: used,
        backend_name: used.short_name().into(),
        on_device: used.is_on_device(),
        original: text.to_string(),
        unchanged: diff.changes().is_empty(),
        suggestion,
        diff,
        duration_ms: start.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::built_in;

    fn unreachable_config() -> BackendConfig {
        BackendConfig { ollama_url: "http://127.0.0.1:9".into(), ..Default::default() }
    }

    #[tokio::test]
    async fn fix_falls_back_to_built_in_checker() {
        let fix = built_in().into_iter().find(|p| p.id == "fix-grammar").unwrap();
        let result = run(&fix, "This is a tset.", BackendId::Ollama, &unreachable_config(), true).await.unwrap();
        assert_eq!(result.backend, BackendId::BuiltIn);
        assert!(result.on_device);
    }

    #[tokio::test]
    async fn rewrite_needs_an_ai() {
        let improve = built_in().into_iter().find(|p| p.id == "improve").unwrap();
        let err = run(&improve, "hi", BackendId::BuiltIn, &unreachable_config(), true).await.unwrap_err();
        assert_eq!(err, BackendError::Unavailable(NEEDS_AI_MESSAGE.into()));
        let err = run(&improve, "hi", BackendId::Ollama, &unreachable_config(), true).await.unwrap_err();
        assert!(err.means_not_set_up());
    }
}
