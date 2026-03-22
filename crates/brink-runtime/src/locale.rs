//! Locale overlay loading.

use std::collections::HashMap;

use brink_format::{DefinitionId, LineEntry, LocaleData};

use crate::error::RuntimeError;
use crate::program::Program;

/// Controls how missing scopes are handled when applying a locale overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocaleMode {
    /// Every scope in the base must appear in the locale. Missing scopes
    /// produce a `LocaleScopeMissing` error.
    Strict,
    /// Missing scopes keep their base line tables unchanged.
    Overlay,
}

/// Apply a locale overlay to a set of base line tables.
///
/// Returns a new set of line tables with locale content replacing matching
/// scopes. The `Program` is used only for structural metadata (scope IDs,
/// checksum) — it is not mutated.
pub fn apply_locale(
    program: &Program,
    locale: &LocaleData,
    base: &[Vec<LineEntry>],
    mode: LocaleMode,
) -> Result<Vec<Vec<LineEntry>>, RuntimeError> {
    if locale.base_checksum != program.source_checksum {
        return Err(RuntimeError::LocaleChecksumMismatch {
            expected: program.source_checksum,
            actual: locale.base_checksum,
        });
    }

    // Build scope_id → line_tables index.
    let scope_idx_map: HashMap<DefinitionId, usize> = program
        .scope_ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect();

    // Start with a clone of the base tables.
    let mut result = base.to_vec();
    let mut covered = vec![false; program.scope_ids.len()];

    for locale_scope in &locale.line_tables {
        let Some(&idx) = scope_idx_map.get(&locale_scope.scope_id) else {
            return Err(RuntimeError::LocaleScopeNotInBase(locale_scope.scope_id));
        };

        // Convert LocaleLineEntry → LineEntry (source_hash=0 for locale entries).
        let entries: Vec<LineEntry> = locale_scope
            .lines
            .iter()
            .map(|le| LineEntry {
                content: le.content.clone(),
                source_hash: 0,
                audio_ref: le.audio_ref.clone(),
                slot_info: Vec::new(),
                source_location: None,
            })
            .collect();

        result[idx] = entries;
        covered[idx] = true;
    }

    if matches!(mode, LocaleMode::Strict) {
        for (i, was_covered) in covered.iter().enumerate() {
            if !was_covered {
                return Err(RuntimeError::LocaleScopeMissing(program.scope_ids[i]));
            }
        }
    }

    Ok(result)
}
