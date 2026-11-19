// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Presets are plain Markdown files with a small front-matter header, the same
// format as Grammable for macOS, so presets are shared between all versions:
//
// ---
// name: Professional Email
// icon: briefcase
// mode: rewrite            # rewrite | fix
// backend: claude          # optional
// model: sonnet            # optional
// order: 20                # optional
// ---
// Rewrite the text in a clear, polite and professional tone.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backends::BackendId;

/// How the result of a preset is reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresetMode {
    /// Small corrections, shown as individual changes to accept or reject.
    Fix,
    /// Whole-text transformation, shown as before / after.
    Rewrite,
}

impl PresetMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PresetMode::Fix => "fix",
            PresetMode::Rewrite => "rewrite",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    /// Icon name (SF Symbol names from the macOS version; the UI maps them).
    pub icon: String,
    pub mode: PresetMode,
    /// Optional backend override (raw value of `BackendId`).
    pub backend: Option<String>,
    pub model: Option<String>,
    pub order: i32,
    pub instructions: String,
    pub is_built_in: bool,
    pub file: Option<PathBuf>,
}

impl Preset {
    pub fn new(id: &str, name: &str, icon: &str, mode: PresetMode, order: i32, instructions: &str) -> Self {
        Preset {
            id: id.into(),
            name: name.into(),
            icon: icon.into(),
            mode,
            backend: None,
            model: None,
            order,
            instructions: instructions.trim().into(),
            is_built_in: true,
            file: None,
        }
    }

    pub fn backend_id(&self) -> Option<BackendId> {
        self.backend.as_deref().and_then(BackendId::parse)
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PresetError {
    #[error("Preset “{0}” has no instructions below the front matter.")]
    EmptyInstructions(String),
}

/// Parses a preset Markdown file. Only a flat `key: value` subset of YAML is supported.
pub fn parse(text: &str, id: &str, file: Option<&Path>) -> Result<Preset, PresetError> {
    let (meta, body) = split_front_matter(text);
    let instructions = body.trim().to_string();
    if instructions.is_empty() {
        let label = file
            .and_then(|f| f.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| id.to_string());
        return Err(PresetError::EmptyInstructions(label));
    }
    let non_empty = |key: &str| meta.get(key).filter(|v| !v.is_empty()).cloned();

    Ok(Preset {
        id: id.to_string(),
        name: non_empty("name").unwrap_or_else(|| humanize(id)),
        icon: non_empty("icon").unwrap_or_else(|| "wand.and.stars".into()),
        mode: match meta.get("mode").map(|m| m.to_lowercase()).as_deref() {
            Some("fix") => PresetMode::Fix,
            _ => PresetMode::Rewrite,
        },
        backend: non_empty("backend").filter(|b| b != "default").map(|b| b.to_lowercase()),
        model: non_empty("model"),
        order: meta.get("order").and_then(|o| o.parse().ok()).unwrap_or(100),
        instructions,
        is_built_in: false,
        file: file.map(Path::to_path_buf),
    })
}

pub fn serialize(preset: &Preset) -> String {
    let mut lines = vec![
        "---".to_string(),
        format!("name: {}", preset.name),
        format!("icon: {}", preset.icon),
        format!("mode: {}", preset.mode.as_str()),
    ];
    if let Some(b) = &preset.backend {
        lines.push(format!("backend: {b}"));
    }
    if let Some(m) = &preset.model {
        lines.push(format!("model: {m}"));
    }
    lines.push(format!("order: {}", preset.order));
    lines.push("---".into());
    lines.push(preset.instructions.clone());
    lines.join("\n") + "\n"
}

/// Splits `---` front matter from the body. Text without front matter is all body.
pub(crate) fn split_front_matter(text: &str) -> (HashMap<String, String>, String) {
    let normalized = text.replace("\r\n", "\n");
    let is_blank = |l: &str| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}').is_empty();
    let lines: Vec<&str> = normalized.split('\n').skip_while(|l| is_blank(l)).collect();

    let starts = lines
        .first()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}') == "---")
        .unwrap_or(false);
    if !starts {
        return (HashMap::new(), normalized);
    }
    let Some(end) = lines.iter().skip(1).position(|l| l.trim() == "---").map(|p| p + 1) else {
        return (HashMap::new(), normalized);
    };

    let mut meta = HashMap::new();
    for line in &lines[1..end] {
        let Some(colon) = line.find(':') else { continue };
        let key = line[..colon].trim().to_lowercase();
        let mut value = &line[colon + 1..];
        // Strip a trailing ` # comment` (only after whitespace, so "C#" survives).
        if let Some(hash) = value.find(" #") {
            value = &value[..hash];
        }
        let mut value = value.trim().to_string();
        let quoted = value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\'')));
        if quoted {
            value = value[1..value.len() - 1].to_string();
        }
        if !key.is_empty() {
            meta.insert(key, value);
        }
    }
    (meta, lines[end + 1..].join("\n"))
}

pub(crate) fn humanize(id: &str) -> String {
    id.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Presets that ship with the app. A user file with the same id overrides it.
pub fn built_in() -> Vec<Preset> {
    use PresetMode::*;
    vec![
        Preset::new("fix-grammar", "Fix Grammar & Spelling", "checkmark.seal", Fix, 10,
            "Correct spelling, grammar, punctuation, capitalization and word-usage mistakes.\n\
             Make the minimum number of changes needed. Do not rephrase sentences that are already correct.\n\
             Keep the author's tone, style, wording, language and formatting."),
        Preset::new("improve", "Improve Writing", "sparkles", Rewrite, 20,
            "Improve the writing: make it clearer, smoother and easier to read while keeping the same meaning,\n\
             tone and approximate length. Fix any grammar or spelling mistakes."),
        Preset::new("professional", "Make Professional", "briefcase", Rewrite, 30,
            "Rewrite the text in a clear, polite and professional tone suitable for work communication.\n\
             Keep the same meaning and all facts. Avoid jargon and filler."),
        Preset::new("friendly", "Make Friendly", "face.smiling", Rewrite, 40,
            "Rewrite the text in a warm, friendly and approachable tone. Keep it natural and keep the same meaning.\n\
             Do not add emojis unless the original uses them."),
        Preset::new("confident", "Make Confident", "bolt", Rewrite, 50,
            "Rewrite the text so it sounds confident and assertive. Remove hedging words such as\n\
             \"just\", \"maybe\", \"I think\", \"sorry to bother\". Keep it polite and keep the same meaning."),
        Preset::new("concise", "Make Concise", "scissors", Rewrite, 60,
            "Make the text shorter and more direct. Remove redundancy and filler while keeping every key point."),
        Preset::new("simplify", "Simplify", "text.badge.minus", Rewrite, 70,
            "Rewrite the text in plain, simple language that is easy for anyone to understand.\n\
             Use short sentences and common words. Keep the same meaning."),
        Preset::new("bullets", "Format as Bullet Points", "list.bullet", Rewrite, 80,
            "Reformat the text as a clean bulleted list using \"- \" bullets. One idea per bullet.\n\
             Keep the original wording where possible and fix obvious mistakes."),
        Preset::new("email", "Format as Email", "envelope", Rewrite, 90,
            "Turn the text into a well-structured email with a greeting, short clear paragraphs and a closing line.\n\
             Do not invent names, dates or facts; use neutral placeholders like [Name] only when needed."),
    ]
}

/// Loads built-in presets plus user presets from `~/.grammable/presets/*.md`.
pub struct PresetStore {
    pub directory: PathBuf,
}

pub struct LoadResult {
    pub presets: Vec<Preset>,
    pub errors: Vec<String>,
}

impl Default for PresetStore {
    fn default() -> Self {
        PresetStore { directory: Self::default_directory() }
    }
}

impl PresetStore {
    pub fn new(directory: PathBuf) -> Self {
        PresetStore { directory }
    }

    pub fn default_directory() -> PathBuf {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".grammable").join("presets")
    }

    /// Built-in presets, overridden or extended by user files. Sorted by order, then name.
    pub fn load(&self) -> LoadResult {
        let mut by_id: HashMap<String, Preset> = built_in().into_iter().map(|p| (p.id.clone(), p)).collect();
        let mut errors = Vec::new();

        if let Ok(entries) = fs::read_dir(&self.directory) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_md = path.extension().map(|e| e.eq_ignore_ascii_case("md")).unwrap_or(false);
                let name = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
                if !is_md || name.is_empty() || name.starts_with('.') || name == "readme" {
                    continue;
                }
                let text = match fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(e) => {
                        errors.push(format!("{}: {e}", path.display()));
                        continue;
                    }
                };
                match parse(&text, &name, Some(&path)) {
                    Ok(mut preset) => {
                        if let Some(b) = by_id.get(&name) {
                            if b.is_built_in && preset.order == 100 {
                                preset.order = b.order; // keep the menu position when overriding a built-in
                            }
                        }
                        by_id.insert(name, preset);
                    }
                    Err(e) => errors.push(e.to_string()),
                }
            }
        }

        let mut presets: Vec<Preset> = by_id.into_values().collect();
        presets.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        LoadResult { presets, errors }
    }

    /// Creates the presets folder with a short README the first time.
    pub fn ensure_directory(&self) -> std::io::Result<PathBuf> {
        fs::create_dir_all(&self.directory)?;
        let readme = self.directory.join("README.md");
        if !readme.exists() {
            fs::write(&readme, FOLDER_README)?;
        }
        Ok(self.directory.clone())
    }

    /// Writes a built-in preset to `<id>.md` so the user can customize it, and returns the path.
    /// An existing file (already customized) is left alone. `None` when `id` isn't a built-in.
    pub fn export_built_in(&self, id: &str) -> std::io::Result<Option<PathBuf>> {
        let Some(preset) = built_in().into_iter().find(|p| p.id == id) else {
            return Ok(None);
        };
        self.ensure_directory()?;
        let path = self.directory.join(format!("{id}.md"));
        if !path.exists() {
            fs::write(&path, serialize(&preset))?;
        }
        Ok(Some(path))
    }

    /// Writes a new preset file and returns its path. Never overwrites an existing file.
    pub fn create_preset(&self, name: &str) -> std::io::Result<PathBuf> {
        self.ensure_directory()?;
        let base = match slug(name) {
            s if s.is_empty() => "my-preset".to_string(),
            s => s,
        };
        let mut path = self.directory.join(format!("{base}.md"));
        let mut n = 2;
        while path.exists() {
            path = self.directory.join(format!("{base}-{n}.md"));
            n += 1;
        }
        let id = path.file_stem().unwrap().to_string_lossy().into_owned();
        let mut preset = Preset::new(&id, name, "wand.and.stars", PresetMode::Rewrite, 200,
            "Describe what Grammable should do with the selected text.");
        preset.is_built_in = false;
        fs::write(&path, serialize(&preset))?;
        Ok(path)
    }
}

const FOLDER_README: &str = r#"# Grammable presets

Every `.md` file in this folder becomes an action in the Grammable menu.
A file named like a built-in preset (for example `fix-grammar.md`) replaces it.

```markdown
---
name: Professional Email
icon: briefcase          # icon name
mode: rewrite            # rewrite = before/after view, fix = accept/reject each change
backend: claude          # optional: claude | codex | gemini | antigravity | devin | ollama | openai-compatible | builtin
model: sonnet            # optional, depends on the backend
order: 20                # optional, position in the menu
---
Rewrite the text in a clear, polite and professional tone.
```

Share your best presets: https://github.com/UdayaLakmal/Grammable/tree/main/presets
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_front_matter() {
        let text = "\u{FEFF}\n---\nname: \"Pro Email\"\nmode: fix # comment\nbackend: Ollama\norder: 5\nicon: C#\n---\n\nBe nice.\n";
        let p = parse(text, "pro-email", None).unwrap();
        assert_eq!(p.name, "Pro Email");
        assert_eq!(p.mode, PresetMode::Fix);
        assert_eq!(p.backend.as_deref(), Some("ollama"));
        assert_eq!(p.order, 5);
        assert_eq!(p.icon, "C#");
        assert_eq!(p.instructions, "Be nice.");
    }

    #[test]
    fn body_without_front_matter_uses_defaults() {
        let p = parse("Translate to French.", "to-french", None).unwrap();
        assert_eq!(p.name, "To French");
        assert_eq!(p.mode, PresetMode::Rewrite);
        assert_eq!(p.order, 100);
        assert!(p.backend.is_none());
    }

    #[test]
    fn empty_instructions_fail() {
        assert!(parse("---\nname: x\n---\n  \n", "x", None).is_err());
    }

    #[test]
    fn default_backend_means_none() {
        let p = parse("---\nbackend: default\n---\nHi", "x", None).unwrap();
        assert!(p.backend.is_none());
    }

    #[test]
    fn serialize_round_trips() {
        let mut p = built_in()[1].clone();
        p.backend = Some("claude".into());
        let back = parse(&serialize(&p), &p.id, None).unwrap();
        assert_eq!(back.name, p.name);
        assert_eq!(back.mode, p.mode);
        assert_eq!(back.backend, p.backend);
        assert_eq!(back.instructions, p.instructions);
    }

    #[test]
    fn shared_presets_from_the_macos_repo_parse() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../presets");
        let mut count = 0;
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let stem = path.file_stem().unwrap().to_string_lossy().to_lowercase();
            if stem == "readme" || path.extension().map(|e| e != "md").unwrap_or(true) {
                continue;
            }
            parse(&fs::read_to_string(&path).unwrap(), &stem, Some(&path)).unwrap();
            count += 1;
        }
        assert!(count > 0);
    }

    #[test]
    fn store_overrides_built_in_and_keeps_order() {
        let dir = std::env::temp_dir().join(format!("grammable-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("fix-grammar.md"), "---\nname: My Fix\nmode: fix\n---\nFix it.").unwrap();
        fs::write(dir.join("broken.md"), "---\nname: Broken\n---\n").unwrap();
        let result = PresetStore::new(dir.clone()).load();
        let fix = result.presets.iter().find(|p| p.id == "fix-grammar").unwrap();
        assert_eq!(fix.name, "My Fix");
        assert_eq!(fix.order, 10);
        assert_eq!(result.errors.len(), 1);
        let created = PresetStore::new(dir.clone()).create_preset("Pirate Speak!").unwrap();
        assert!(created.ends_with("pirate-speak.md"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_built_in_writes_once() {
        let dir = std::env::temp_dir().join(format!("grammable-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = PresetStore::new(dir.clone());
        assert_eq!(store.export_built_in("nope").unwrap(), None);
        let path = store.export_built_in("concise").unwrap().unwrap();
        let exported = parse(&fs::read_to_string(&path).unwrap(), "concise", Some(&path)).unwrap();
        assert_eq!(exported.name, "Make Concise");
        assert_eq!(exported.order, 60);
        fs::write(&path, "---\nname: Mine\n---\nShorter.").unwrap();
        store.export_built_in("concise").unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("Mine"));
        let _ = fs::remove_dir_all(&dir);
    }
}
