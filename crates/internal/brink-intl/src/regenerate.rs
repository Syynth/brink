//! Regeneration logic for preserving translations across recompilation.
//!
//! When source `.ink` files are recompiled, existing translations must be
//! preserved. This module diffs a new export against an existing translated
//! `lines.json` and produces an updated file with translations carried forward.

use std::collections::HashMap;

use crate::align::{Alignment, align_hashes};
use crate::json_model::{LineJson, LinesJson, ScopeJson};

/// Regenerate a `lines.json` by merging translations from `existing` into the
/// structure of `new_export`.
///
/// Lines matched by hash retain their translated content and audio. New lines
/// (insertions) have `content: None`. Deleted lines are dropped. Edited lines
/// (adjacent remove+insert at the same position) carry the old translation
/// forward with the new hash, signaling implicit `needs_review`.
pub fn regenerate_lines(new_export: &LinesJson, existing: &LinesJson) -> LinesJson {
    let old_scope_map: HashMap<&str, &ScopeJson> =
        existing.scopes.iter().map(|s| (s.id.as_str(), s)).collect();

    let scopes = new_export
        .scopes
        .iter()
        .map(|new_scope| {
            let lines = if let Some(old_scope) = old_scope_map.get(new_scope.id.as_str()) {
                regenerate_scope_lines(&new_scope.lines, &old_scope.lines)
            } else {
                // Entirely new scope — all lines untranslated.
                new_scope
                    .lines
                    .iter()
                    .map(|line| LineJson {
                        content: None,
                        ..line.clone()
                    })
                    .collect()
            };

            ScopeJson {
                name: new_scope.name.clone(),
                id: new_scope.id.clone(),
                lines,
            }
        })
        .collect();

    LinesJson {
        version: new_export.version,
        source_checksum: new_export.source_checksum.clone(),
        scopes,
    }
}

/// Regenerate lines within a single scope using LCS alignment.
fn regenerate_scope_lines(new_lines: &[LineJson], old_lines: &[LineJson]) -> Vec<LineJson> {
    let old_hashes: Vec<&str> = old_lines.iter().map(|l| l.hash.as_str()).collect();
    let new_hashes: Vec<&str> = new_lines.iter().map(|l| l.hash.as_str()).collect();
    let alignment = align_hashes(&old_hashes, &new_hashes);

    // First pass: build raw aligned entries.
    let mut result: Vec<LineJson> = Vec::with_capacity(new_lines.len());

    // Track pending Removed entries for edit detection.
    let mut pending_removed: Option<&LineJson> = None;

    for entry in &alignment {
        match entry {
            Alignment::Matched { old_idx, new_idx } => {
                // Flush any pending removed (it was a true deletion, not an edit).
                pending_removed = None;

                // Carry translation from old, use new index and hash.
                let old_line = &old_lines[*old_idx];
                let new_line = &new_lines[*new_idx];
                result.push(LineJson {
                    index: new_line.index,
                    content: old_line.content.clone(),
                    hash: new_line.hash.clone(),
                    audio: old_line.audio.clone(),
                    slots: Vec::new(),
                    source: None,
                });
            }
            Alignment::Removed { old_idx } => {
                // Buffer it — might be part of an edit pair.
                pending_removed = Some(&old_lines[*old_idx]);
            }
            Alignment::Inserted { new_idx } => {
                let new_line = &new_lines[*new_idx];

                if let Some(removed) = pending_removed.take() {
                    // Edit heuristic: adjacent Removed+Inserted → carry old translation.
                    result.push(LineJson {
                        index: new_line.index,
                        content: removed.content.clone(),
                        hash: new_line.hash.clone(),
                        audio: removed.audio.clone(),
                        slots: Vec::new(),
                        source: None,
                    });
                } else {
                    // Pure insertion — no translation available.
                    result.push(LineJson {
                        index: new_line.index,
                        content: None,
                        hash: new_line.hash.clone(),
                        audio: None,
                        slots: Vec::new(),
                        source: None,
                    });
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_line(index: u16, hash: &str, content: Option<&str>, audio: Option<&str>) -> LineJson {
        use crate::json_model::ContentJson;
        LineJson {
            index,
            content: content.map(|s| ContentJson::Plain(s.to_string())),
            hash: hash.to_string(),
            audio: audio.map(str::to_string),
            slots: Vec::new(),
            source: None,
        }
    }

    fn make_scope(id: &str, lines: Vec<LineJson>) -> ScopeJson {
        ScopeJson {
            name: None,
            id: id.to_string(),
            lines,
        }
    }

    fn make_lines_json(scopes: Vec<ScopeJson>) -> LinesJson {
        LinesJson {
            version: 1,
            source_checksum: "0x00000000".to_string(),
            scopes,
        }
    }

    #[test]
    fn identity_preserves_translations() {
        let existing = make_lines_json(vec![make_scope(
            "0x01",
            vec![
                make_line(0, "aaa", Some("Hello"), None),
                make_line(1, "bbb", Some("World"), None),
            ],
        )]);
        let new_export = make_lines_json(vec![make_scope(
            "0x01",
            vec![
                make_line(0, "aaa", Some("Hello (source)"), None),
                make_line(1, "bbb", Some("World (source)"), None),
            ],
        )]);

        let result = regenerate_lines(&new_export, &existing);
        assert_eq!(result.scopes.len(), 1);
        let lines = &result.scopes[0].lines;
        assert_eq!(lines.len(), 2);
        // Translations from existing are preserved, not overwritten by new_export.
        assert_eq!(lines[0].content, existing.scopes[0].lines[0].content);
        assert_eq!(lines[1].content, existing.scopes[0].lines[1].content);
    }

    #[test]
    fn insertion_produces_none_content() {
        let existing = make_lines_json(vec![make_scope(
            "0x01",
            vec![
                make_line(0, "aaa", Some("Hello"), None),
                make_line(1, "ccc", Some("!"), None),
            ],
        )]);
        let new_export = make_lines_json(vec![make_scope(
            "0x01",
            vec![
                make_line(0, "aaa", Some("Hello (src)"), None),
                make_line(1, "bbb", Some("World (src)"), None), // new
                make_line(2, "ccc", Some("! (src)"), None),
            ],
        )]);

        let result = regenerate_lines(&new_export, &existing);
        let lines = &result.scopes[0].lines;
        assert_eq!(lines.len(), 3);
        assert!(lines[0].content.is_some()); // preserved
        assert!(lines[1].content.is_none()); // new, untranslated
        assert!(lines[2].content.is_some()); // preserved
    }

    #[test]
    fn deletion_drops_line() {
        let existing = make_lines_json(vec![make_scope(
            "0x01",
            vec![
                make_line(0, "aaa", Some("Hello"), None),
                make_line(1, "bbb", Some("World"), None),
                make_line(2, "ccc", Some("!"), None),
            ],
        )]);
        let new_export = make_lines_json(vec![make_scope(
            "0x01",
            vec![
                make_line(0, "aaa", Some("Hello (src)"), None),
                make_line(1, "ccc", Some("! (src)"), None),
            ],
        )]);

        let result = regenerate_lines(&new_export, &existing);
        let lines = &result.scopes[0].lines;
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].content, existing.scopes[0].lines[0].content);
        assert_eq!(lines[1].content, existing.scopes[0].lines[2].content);
    }

    #[test]
    fn edit_carries_old_translation_with_new_hash() {
        let existing = make_lines_json(vec![make_scope(
            "0x01",
            vec![make_line(0, "aaa", Some("Translated text"), None)],
        )]);
        let new_export = make_lines_json(vec![make_scope(
            "0x01",
            vec![make_line(0, "xxx", Some("Changed source"), None)],
        )]);

        let result = regenerate_lines(&new_export, &existing);
        let line = &result.scopes[0].lines[0];
        // Old translation preserved.
        assert_eq!(line.content, existing.scopes[0].lines[0].content);
        // But new hash signals needs_review.
        assert_eq!(line.hash, "xxx");
    }

    #[test]
    fn new_scope_all_untranslated() {
        let existing = make_lines_json(vec![]);
        let new_export = make_lines_json(vec![make_scope(
            "0x02",
            vec![make_line(0, "aaa", Some("Hello"), None)],
        )]);

        let result = regenerate_lines(&new_export, &existing);
        assert_eq!(result.scopes.len(), 1);
        assert!(result.scopes[0].lines[0].content.is_none());
    }

    #[test]
    fn removed_scope_dropped() {
        let existing = make_lines_json(vec![make_scope(
            "0x01",
            vec![make_line(0, "aaa", Some("Hello"), None)],
        )]);
        let new_export = make_lines_json(vec![]);

        let result = regenerate_lines(&new_export, &existing);
        assert!(result.scopes.is_empty());
    }

    #[test]
    fn audio_preserved_through_match() {
        let existing = make_lines_json(vec![make_scope(
            "0x01",
            vec![make_line(0, "aaa", Some("Hello"), Some("audio/hi.wav"))],
        )]);
        let new_export = make_lines_json(vec![make_scope(
            "0x01",
            vec![make_line(0, "aaa", Some("Hello (src)"), None)],
        )]);

        let result = regenerate_lines(&new_export, &existing);
        assert_eq!(
            result.scopes[0].lines[0].audio,
            Some("audio/hi.wav".to_string())
        );
    }

    #[test]
    fn version_and_checksum_from_new() {
        let existing = make_lines_json(vec![]);
        let mut new_export = make_lines_json(vec![]);
        new_export.version = 2;
        new_export.source_checksum = "0xdeadbeef".to_string();

        let result = regenerate_lines(&new_export, &existing);
        assert_eq!(result.version, 2);
        assert_eq!(result.source_checksum, "0xdeadbeef");
    }
}
