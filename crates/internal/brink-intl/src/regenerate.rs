//! Regeneration logic for preserving translations across recompilation.
//!
//! When source `.ink` files are recompiled, existing translations must be
//! preserved. This module diffs a new export against an existing translated
//! `lines.json` and produces an updated file with translations carried forward.

use std::collections::{BTreeSet, HashMap};

use brink_format::AliasEntry;

use crate::align::{Alignment, align_hashes};
use crate::json_model::{LineJson, LinesJson, ScopeJson};
use crate::scope_alias::{ScopeAliasIndex, format_scope_id, parse_scope_id_lenient};

/// Regenerate a `lines.json` by merging translations from `existing` into the
/// structure of `new_export`.
///
/// Lines matched by hash retain their translated content and audio. New lines
/// (insertions) have `content: None`. Deleted lines are dropped. Edited lines
/// (adjacent remove+insert at the same position) carry the old translation
/// forward with the new hash, signaling implicit `needs_review`.
///
/// `aliases` is the compiled `#@was` alias table of the story `new_export`
/// came from (`StoryData::alias_table`). A scope whose id moved because it —
/// or an ancestor — was renamed no longer matches `existing` by id, so it is
/// looked up under its declared pre-rename ids instead, and its translations
/// carry forward (#1442). Pass an empty slice when no alias edges are
/// available; matching then behaves exactly as it did before rebinding
/// existed. The regenerated file is keyed on the *new* ids, so the rebind is
/// a one-time cost and the `#@was` directive stays deletable after its
/// migration window (`docs/modules-spec.md` §5).
pub fn regenerate_lines(
    new_export: &LinesJson,
    existing: &LinesJson,
    aliases: &[AliasEntry],
) -> LinesJson {
    let old_scope_map: HashMap<&str, &ScopeJson> =
        existing.scopes.iter().map(|s| (s.id.as_str(), s)).collect();
    let alias_index = ScopeAliasIndex::new(aliases);

    // A direct id match always wins, so resolve those first and record which
    // old scopes they consumed; a rebind may then only claim what is left.
    let claimed: BTreeSet<&str> = new_export
        .scopes
        .iter()
        .filter(|s| old_scope_map.contains_key(s.id.as_str()))
        .map(|s| s.id.as_str())
        .collect();

    let scopes = new_export
        .scopes
        .iter()
        .map(|new_scope| {
            let matched = old_scope_map
                .get(new_scope.id.as_str())
                .copied()
                .or_else(|| find_renamed_scope(new_scope, &old_scope_map, &alias_index, &claimed));

            let lines = if let Some(old_scope) = matched {
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

/// Find the pre-rename scope in `existing` that `new_scope` used to be,
/// following `#@was` alias edges backwards.
///
/// Candidates are ascending by id ([`ScopeAliasIndex::previous`]) so the
/// choice is deterministic when a definition absorbed more than one declared
/// rename, and already-claimed scopes are skipped so a rebind can never steal
/// translations from a direct match.
fn find_renamed_scope<'a>(
    new_scope: &ScopeJson,
    old_scope_map: &HashMap<&str, &'a ScopeJson>,
    aliases: &ScopeAliasIndex,
    claimed: &BTreeSet<&str>,
) -> Option<&'a ScopeJson> {
    if aliases.is_empty() {
        return None;
    }
    // A non-canonical id (a legacy XLIFF `<file id>` display-name fallback)
    // simply does not participate in rebinding.
    let new_id = parse_scope_id_lenient(&new_scope.id)?;
    aliases.previous(new_id).iter().find_map(|old_id| {
        let old_key = format_scope_id(*old_id);
        if claimed.contains(old_key.as_str()) {
            return None;
        }
        old_scope_map.get(old_key.as_str()).copied()
    })
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

    fn make_named_scope(id: &str, name: &str, lines: Vec<LineJson>) -> ScopeJson {
        ScopeJson {
            name: Some(name.to_string()),
            id: id.to_string(),
            lines,
        }
    }

    /// A canonical scope id — the `0x{:016x}` spelling `export_lines` emits.
    fn scope_id(raw: u64) -> String {
        format_scope_id(brink_format::DefinitionId::new(
            brink_format::DefinitionTag::Address,
            raw,
        ))
    }

    /// The `old -> new` edge `#@was` compiles to.
    fn alias(old: u64, new: u64) -> AliasEntry {
        AliasEntry {
            old: brink_format::DefinitionId::new(brink_format::DefinitionTag::Address, old),
            new: brink_format::DefinitionId::new(brink_format::DefinitionTag::Address, new),
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

        let result = regenerate_lines(&new_export, &existing, &[]);
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

        let result = regenerate_lines(&new_export, &existing, &[]);
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

        let result = regenerate_lines(&new_export, &existing, &[]);
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

        let result = regenerate_lines(&new_export, &existing, &[]);
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

        let result = regenerate_lines(&new_export, &existing, &[]);
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

        let result = regenerate_lines(&new_export, &existing, &[]);
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

        let result = regenerate_lines(&new_export, &existing, &[]);
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

        let result = regenerate_lines(&new_export, &existing, &[]);
        assert_eq!(result.version, 2);
        assert_eq!(result.source_checksum, "0xdeadbeef");
    }

    // ── #1442: rebinding a scope whose id moved under a declared rename ──

    #[test]
    fn alias_rebinds_a_renamed_scope() {
        let existing = make_lines_json(vec![make_named_scope(
            &scope_id(1),
            "hub",
            vec![make_line(0, "aaa", Some("Hola"), Some("audio/hi.wav"))],
        )]);
        let new_export = make_lines_json(vec![make_named_scope(
            &scope_id(2),
            "plaza",
            vec![make_line(0, "aaa", Some("Hello"), None)],
        )]);

        // Without the edge the scope reads as brand new.
        let blind = regenerate_lines(&new_export, &existing, &[]);
        assert!(blind.scopes[0].lines[0].content.is_none());

        let rebound = regenerate_lines(&new_export, &existing, &[alias(1, 2)]);
        assert_eq!(
            rebound.scopes[0].lines[0].content,
            existing.scopes[0].lines[0].content
        );
        assert_eq!(
            rebound.scopes[0].lines[0].audio,
            Some("audio/hi.wav".to_string())
        );
        // Output is keyed on the *new* id, so the rebind is a one-time cost.
        assert_eq!(rebound.scopes[0].id, scope_id(2));
        assert_eq!(rebound.scopes[0].name.as_deref(), Some("plaza"));
    }

    /// Root content is an unnamed scope (`export.rs` resolves scope names
    /// through an `Option`), so anonymous scopes reach the translation file
    /// too. Rebinding is keyed on the id alone and never consults the name.
    #[test]
    fn anonymous_scope_rebinds_by_id_alone() {
        let existing = make_lines_json(vec![make_scope(
            &scope_id(1),
            vec![make_line(0, "aaa", Some("Hola"), None)],
        )]);
        let new_export = make_lines_json(vec![make_scope(
            &scope_id(2),
            vec![make_line(0, "aaa", Some("Hello"), None)],
        )]);

        let rebound = regenerate_lines(&new_export, &existing, &[alias(1, 2)]);
        assert_eq!(
            rebound.scopes[0].lines[0].content,
            existing.scopes[0].lines[0].content
        );
        assert!(rebound.scopes[0].name.is_none());
    }

    /// A direct id match must win: an already-regenerated file carries the
    /// post-rename id, and a stale entry under the pre-rename id must not
    /// override it.
    #[test]
    fn direct_match_wins_over_alias_rebind() {
        let existing = make_lines_json(vec![
            make_scope(&scope_id(1), vec![make_line(0, "aaa", Some("STALE"), None)]),
            make_scope(
                &scope_id(2),
                vec![make_line(0, "aaa", Some("CURRENT"), None)],
            ),
        ]);
        let new_export = make_lines_json(vec![make_scope(
            &scope_id(2),
            vec![make_line(0, "aaa", Some("Hello"), None)],
        )]);

        let result = regenerate_lines(&new_export, &existing, &[alias(1, 2)]);
        assert_eq!(
            result.scopes[0].lines[0].content,
            existing.scopes[1].lines[0].content
        );
    }

    /// Two new scopes cannot both claim the same old scope: the one that
    /// matches directly keeps it, and the rebind finds nothing.
    #[test]
    fn a_rebind_never_steals_a_directly_matched_scope() {
        let existing = make_lines_json(vec![make_scope(
            &scope_id(1),
            vec![make_line(0, "aaa", Some("Hola"), None)],
        )]);
        let new_export = make_lines_json(vec![
            make_scope(&scope_id(1), vec![make_line(0, "aaa", Some("Hello"), None)]),
            make_scope(&scope_id(2), vec![make_line(0, "aaa", Some("Hello"), None)]),
        ]);

        let result = regenerate_lines(&new_export, &existing, &[alias(1, 2)]);
        assert_eq!(
            result.scopes[0].lines[0].content,
            existing.scopes[0].lines[0].content,
            "the direct match keeps the translation"
        );
        assert!(
            result.scopes[1].lines[0].content.is_none(),
            "the rebind must not duplicate an already-claimed scope"
        );
    }

    /// An XLIFF that predates the `brink:scope-id` extension falls back to
    /// `<file id>`, a display name, which is not a parseable scope id. Such a
    /// scope simply does not rebind — it must not panic or error.
    #[test]
    fn non_canonical_scope_id_does_not_participate_in_rebinding() {
        let existing = make_lines_json(vec![make_scope(
            "intro",
            vec![make_line(0, "aaa", Some("Hola"), None)],
        )]);
        let new_export = make_lines_json(vec![make_scope(
            "prologue",
            vec![make_line(0, "aaa", Some("Hello"), None)],
        )]);

        let result = regenerate_lines(&new_export, &existing, &[alias(1, 2)]);
        assert!(result.scopes[0].lines[0].content.is_none());
    }
}
