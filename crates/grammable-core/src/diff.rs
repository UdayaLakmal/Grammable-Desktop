// Grammable — Copyright (C) 2026 Udaya Lakmal — GPL-3.0-or-later
//
// A word-level diff that can be partially applied: each change can be accepted
// or rejected on its own.

use serde::Serialize;
use similar::{capture_diff_slices, Algorithm, DiffOp};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextChange {
    pub id: usize,
    pub original: String,
    pub replacement: String,
}

impl TextChange {
    /// Text was added where there was none.
    pub fn is_insertion(&self) -> bool {
        self.original.is_empty() && !self.replacement.is_empty()
    }

    /// Text was removed with nothing in its place.
    pub fn is_deletion(&self) -> bool {
        !self.original.is_empty() && self.replacement.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Segment {
    Same { text: String },
    Change { change: TextChange },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextDiff {
    pub segments: Vec<Segment>,
}

impl TextDiff {
    pub fn new(original: &str, revised: &str) -> Self {
        let old = tokenize(original);
        let new = tokenize(revised);
        let ops = capture_diff_slices(Algorithm::Myers, &old, &new);

        let mut segments = Vec::new();
        let mut same = String::new();
        let (mut from, mut to) = (String::new(), String::new());
        let mut next_id = 0;

        let flush_change = |segments: &mut Vec<Segment>, from: &mut String, to: &mut String, next_id: &mut usize| {
            if !from.is_empty() || !to.is_empty() {
                segments.push(Segment::Change {
                    change: TextChange { id: *next_id, original: std::mem::take(from), replacement: std::mem::take(to) },
                });
                *next_id += 1;
            }
        };

        for op in ops {
            match op {
                DiffOp::Equal { old_index, len, .. } => {
                    flush_change(&mut segments, &mut from, &mut to, &mut next_id);
                    same.extend(old[old_index..old_index + len].iter().map(String::as_str));
                }
                other => {
                    if !same.is_empty() {
                        segments.push(Segment::Same { text: std::mem::take(&mut same) });
                    }
                    let (_, old_range, new_range) = other.as_tag_tuple();
                    from.extend(old[old_range].iter().map(String::as_str));
                    to.extend(new[new_range].iter().map(String::as_str));
                }
            }
        }
        flush_change(&mut segments, &mut from, &mut to, &mut next_id);
        if !same.is_empty() {
            segments.push(Segment::Same { text: same });
        }
        TextDiff { segments }
    }

    pub fn changes(&self) -> Vec<&TextChange> {
        self.segments
            .iter()
            .filter_map(|s| match s {
                Segment::Change { change } => Some(change),
                _ => None,
            })
            .collect()
    }

    /// Builds the final text, applying only the accepted change ids.
    pub fn apply(&self, accepted: &HashSet<usize>) -> String {
        self.segments
            .iter()
            .map(|s| match s {
                Segment::Same { text } => text.as_str(),
                Segment::Change { change } if accepted.contains(&change.id) => change.replacement.as_str(),
                Segment::Change { change } => change.original.as_str(),
            })
            .collect()
    }
}

/// Splits text into words (letters, digits, apostrophes), whitespace runs and single symbols.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    #[derive(PartialEq, Clone, Copy)]
    enum Kind {
        Word,
        Space,
        Other,
    }
    let kind = |c: char| {
        if c.is_alphanumeric() || c == '\'' || c == '’' {
            Kind::Word
        } else if c.is_whitespace() {
            Kind::Space
        } else {
            Kind::Other
        }
    };

    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_kind: Option<Kind> = None;
    for c in text.chars() {
        let k = kind(c);
        if k == Kind::Other {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push(c.to_string());
            current_kind = None;
        } else if Some(k) == current_kind {
            current.push(c);
        } else {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            current.push(c);
            current_kind = Some(k);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_words_spaces_and_symbols() {
        assert_eq!(tokenize("It's  ok, 2 go!"), vec!["It's", "  ", "ok", ",", " ", "2", " ", "go", "!"]);
    }

    #[test]
    fn finds_individual_changes() {
        let d = TextDiff::new("This are a tset.", "This is a test.");
        let changes = d.changes();
        assert_eq!(changes.len(), 2);
        assert_eq!(changes[0].original, "are");
        assert_eq!(changes[0].replacement, "is");
        assert_eq!(changes[1].original, "tset");
        assert_eq!(changes[1].replacement, "test");
    }

    #[test]
    fn partial_apply() {
        let d = TextDiff::new("This are a tset.", "This is a test.");
        assert_eq!(d.apply(&HashSet::new()), "This are a tset.");
        assert_eq!(d.apply(&[0, 1].into_iter().collect()), "This is a test.");
        assert_eq!(d.apply(&[1].into_iter().collect()), "This are a test.");
    }

    #[test]
    fn identical_text_has_no_changes() {
        assert!(TextDiff::new("Same text.", "Same text.").changes().is_empty());
    }

    #[test]
    fn insertions_and_deletions() {
        let d = TextDiff::new("Hello world", "Hello, big world!");
        assert_eq!(d.apply(&d.changes().iter().map(|c| c.id).collect()), "Hello, big world!");
        assert_eq!(d.apply(&HashSet::new()), "Hello world");
    }
}
