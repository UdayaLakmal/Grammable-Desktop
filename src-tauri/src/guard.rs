// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// When Grammable must not act, and what to tell the user.

use crate::platform::Target;
use crate::settings::Settings;

/// Longer selections are refused: too slow and costly to send to an AI.
pub const MAX_SELECTION: usize = 30_000;

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    /// Do nothing at all (excluded app, Grammable itself).
    Ignore,
    /// Tell the user why.
    Refuse { title: String, detail: String },
}

/// Before reading anything: is this window off limits?
pub fn check_target(target: Option<&Target>, settings: &Settings) -> Verdict {
    let Some(target) = target else { return Verdict::Ok };
    if target.is_self() {
        return Verdict::Ignore;
    }
    match &target.process {
        Some(process) if settings.is_excluded(process) => Verdict::Ignore,
        _ => Verdict::Ok,
    }
}

/// After reading: is this text something to work on?
pub fn check_text(text: Option<&str>, settings: &Settings) -> Verdict {
    let trimmed = text.map(str::trim).unwrap_or("");
    if trimmed.is_empty() || trimmed.chars().count() < settings.min_selection_length.max(1) {
        let wayland = crate::commands::session_type() == "wayland";
        return Verdict::Refuse {
            title: "Select some text first".into(),
            detail: if wayland {
                format!(
                    "Highlight text, then press {}. On Wayland this works in X11 apps for now; \
                     full Wayland support is coming.",
                    settings.hotkey
                )
            } else {
                format!("Highlight text in any app, then press {}.", settings.hotkey)
            },
        };
    }
    if text.map_or(0, |t| t.chars().count()) > MAX_SELECTION {
        return Verdict::Refuse {
            title: "That selection is too long".into(),
            detail: format!("Grammable works on up to {MAX_SELECTION} characters at a time. Select a shorter part."),
        };
    }
    Verdict::Ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(process: &str, pid: u32) -> Target {
        Target { handle: 1, pid: Some(pid), process: Some(process.into()) }
    }

    #[test]
    fn excluded_and_own_windows_are_ignored() {
        let s = Settings { excluded_apps: vec!["KeePassXC".into()], ..Settings::default() };
        assert_eq!(check_target(Some(&target("keepassxc", 1)), &s), Verdict::Ignore);
        assert_eq!(check_target(Some(&target("KeePassXC.exe", 1)), &s), Verdict::Ignore);
        assert_eq!(check_target(Some(&target("gedit", std::process::id())), &s), Verdict::Ignore);
        assert_eq!(check_target(Some(&target("gedit", 1)), &s), Verdict::Ok);
        assert_eq!(check_target(None, &s), Verdict::Ok);
    }

    #[test]
    fn text_limits() {
        let s = Settings { min_selection_length: 3, ..Settings::default() };
        assert!(matches!(check_text(None, &s), Verdict::Refuse { .. }));
        assert!(matches!(check_text(Some("  hi \n"), &s), Verdict::Refuse { .. }));
        assert_eq!(check_text(Some(" hey "), &s), Verdict::Ok);
        let long = "a".repeat(MAX_SELECTION + 1);
        match check_text(Some(&long), &s) {
            Verdict::Refuse { title, .. } => assert!(title.contains("too long")),
            other => panic!("{other:?}"),
        }
    }
}
