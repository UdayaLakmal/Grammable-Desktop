// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// Offline spelling and grammar checker built into Grammable, using Harper
// (https://github.com/Automattic/harper). Replaces the macOS NSSpellChecker backend.
// Returns the corrected text itself; there is no model chatter to clean.
//
// NOTE: written against harper-core 3.x from memory and NOT compiled yet.
// Check the API names (FstDictionary::curated, LintGroup::new_curated,
// Document::new_plain_english_curated, Suggestion::apply) on first build.

use harper_core::linting::{LintGroup, Linter, Suggestion};
use harper_core::spell::FstDictionary;
use harper_core::{Dialect, Document};

use super::BackendError;

pub fn check(text: &str) -> Result<String, BackendError> {
    let dictionary = FstDictionary::curated();
    let mut linter = LintGroup::new_curated(dictionary, Dialect::American);
    let document = Document::new_plain_english_curated(text);
    let mut lints = linter.lint(&document);

    // Apply the first suggestion of each lint from the end of the text backwards,
    // skipping overlapping spans so earlier offsets stay valid.
    lints.sort_by_key(|l| std::cmp::Reverse(l.span.start));
    let mut chars: Vec<char> = text.chars().collect();
    let mut last_start = usize::MAX;
    for lint in lints {
        if lint.span.end > last_start {
            continue;
        }
        if let Some(suggestion) = lint.suggestions.first() {
            if matches!(suggestion, Suggestion::ReplaceWith(_) | Suggestion::Remove | Suggestion::InsertAfter(_)) {
                suggestion.apply(lint.span, &mut chars);
                last_start = lint.span.start;
            }
        }
    }
    Ok(chars.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixes_a_spelling_mistake() {
        let fixed = check("This is a tset.").unwrap();
        assert_ne!(fixed, "This is a tset.");
    }

    #[test]
    fn leaves_correct_text_alone() {
        assert_eq!(check("This is a test.").unwrap(), "This is a test.");
    }
}
