//! Locale overlay loading for linked programs.

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

impl Program {
    /// Apply a locale overlay, replacing line table content for matching scopes.
    pub fn apply_locale(
        &mut self,
        locale: &LocaleData,
        mode: LocaleMode,
    ) -> Result<(), RuntimeError> {
        // Validate checksum.
        if locale.base_checksum != self.source_checksum {
            return Err(RuntimeError::LocaleChecksumMismatch {
                expected: self.source_checksum,
                actual: locale.base_checksum,
            });
        }

        // Build scope_id → line_tables index.
        let scope_idx_map: HashMap<DefinitionId, usize> = self
            .scope_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();

        // Track which base scopes were covered (for strict mode).
        let mut covered = vec![false; self.scope_ids.len()];

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
                })
                .collect();

            self.line_tables[idx] = entries;
            covered[idx] = true;
        }

        if matches!(mode, LocaleMode::Strict) {
            for (i, was_covered) in covered.iter().enumerate() {
                if !was_covered {
                    return Err(RuntimeError::LocaleScopeMissing(self.scope_ids[i]));
                }
            }
        }

        Ok(())
    }
}
