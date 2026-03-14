//! High-level XLIFF operations composing export, compile, and regenerate.

use std::collections::HashMap;

use brink_format::StoryData;
use xliff2::{Document, State, SubUnit};

use crate::compile::compile_locale;
use crate::error::IntlError;
use crate::export::export_lines;
use crate::regenerate::regenerate_lines;
use crate::xliff_convert::{lines_json_to_xliff, xliff_to_lines_json};

/// Export a story's line tables as an XLIFF 2.0 document.
///
/// This is the XLIFF equivalent of `export_lines` → JSON serialization.
pub fn generate_locale(story: &StoryData, source_checksum: u32, source_lang: &str) -> Document {
    let lines = export_lines(story, source_checksum);
    lines_json_to_xliff(&lines, source_lang)
}

/// Compile a translated XLIFF document into `.inkl` locale overlay bytes.
///
/// Converts the XLIFF back to `LinesJson`, then delegates to `compile_locale`.
pub fn compile_locale_xliff(
    base_inkb: &[u8],
    doc: &Document,
    locale_tag: &str,
) -> Result<Vec<u8>, IntlError> {
    let lines = xliff_to_lines_json(doc)?;
    compile_locale(base_inkb, &lines, locale_tag)
}

/// Regenerate an XLIFF document after recompilation, preserving translations.
///
/// Produces a fresh export from the new story, converts the existing XLIFF to
/// `LinesJson`, runs `regenerate_lines`, and converts back to XLIFF.
/// Translations with matching hashes keep their state; edited lines get
/// `state="initial"` to signal review.
pub fn regenerate_locale(
    story: &StoryData,
    source_checksum: u32,
    source_lang: &str,
    existing: &Document,
) -> Result<Document, IntlError> {
    let new_export = export_lines(story, source_checksum);
    let existing_lines = xliff_to_lines_json(existing)?;
    let merged = regenerate_lines(&new_export, &existing_lines);

    let mut doc = lines_json_to_xliff(&merged, source_lang);

    // Carry forward target language from existing document.
    if doc.trg_lang.is_none() {
        doc.trg_lang.clone_from(&existing.trg_lang);
    }

    // Build hash→state map from existing XLIFF for state restoration.
    let state_map = build_state_map(existing);

    // Restore states on matched lines and set targets.
    for (file, merged_scope) in doc.files.iter_mut().zip(&merged.scopes) {
        for (unit, merged_line) in file.units.iter_mut().zip(&merged_scope.lines) {
            for su in &mut unit.sub_units {
                if let SubUnit::Segment(seg) = su {
                    // If this line has translated content, set the target.
                    if let Some(ref content) = merged_line.content {
                        let (elements, _) = crate::xliff_convert::content_to_inline(content);
                        seg.target = Some(xliff2::Content {
                            lang: None,
                            elements,
                        });

                        // Restore state from existing if hash matches.
                        if let Some(&old_state) = state_map.get(merged_line.hash.as_str()) {
                            seg.state = Some(old_state);
                        } else {
                            // Hash changed — needs review.
                            seg.state = Some(State::Initial);
                        }
                    }
                }
            }
        }
    }

    Ok(doc)
}

/// Build a map from line hash → segment state from an existing XLIFF document.
fn build_state_map(doc: &Document) -> HashMap<String, State> {
    let mut map = HashMap::new();
    for file in &doc.files {
        for unit in &file.units {
            // Extract hash from extension attributes.
            let hash = unit
                .extensions
                .attributes
                .iter()
                .find(|a| a.namespace == "brink" && a.local_name == "hash")
                .map(|a| a.value.clone());

            let state = unit.sub_units.iter().find_map(|su| match su {
                SubUnit::Segment(seg) => seg.state,
                SubUnit::Ignorable(_) => None,
            });

            if let (Some(h), Some(s)) = (hash, state) {
                map.insert(h, s);
            }
        }
    }
    map
}
