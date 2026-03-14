//! Compile translated `lines.json` into a `.inkl` locale overlay.

use std::collections::HashMap;

use brink_format::{
    DefinitionId, LineContent, LinePart, LocaleData, LocaleLineEntry, LocaleScopeTable,
    PluralCategory, SelectKey, read_inkb_index, read_section_line_tables, write_inkl,
};

use crate::error::IntlError;
use crate::json_model::{ContentJson, LinesJson, PartJson};

/// Compile a translated `LinesJson` against a base `.inkb` file into `.inkl` bytes.
///
/// `base_inkb` is the raw `.inkb` bytes. `lines_json` is the deserialized
/// translated lines. `locale_tag` is a BCP 47 locale string (e.g. "es", "ja").
pub fn compile_locale(
    base_inkb: &[u8],
    lines_json: &LinesJson,
    locale_tag: &str,
) -> Result<Vec<u8>, IntlError> {
    if locale_tag.is_empty() {
        return Err(IntlError::InvalidLocaleTag(locale_tag.to_string()));
    }

    let index = read_inkb_index(base_inkb)?;
    let base_tables = read_section_line_tables(base_inkb, &index)?;

    // Build lookup from scope_id → base line count.
    let base_scope_map: HashMap<DefinitionId, usize> = base_tables
        .iter()
        .map(|lt| (lt.scope_id, lt.lines.len()))
        .collect();

    let mut locale_tables = Vec::with_capacity(lines_json.scopes.len());

    for scope in &lines_json.scopes {
        let scope_id = parse_scope_id(&scope.id)?;

        let base_line_count = base_scope_map
            .get(&scope_id)
            .copied()
            .ok_or_else(|| IntlError::ScopeNotInBase(scope.id.clone()))?;

        if scope.lines.len() != base_line_count {
            return Err(IntlError::LineCountMismatch {
                scope_id: scope.id.clone(),
                expected: base_line_count,
                actual: scope.lines.len(),
            });
        }

        let mut lines = Vec::with_capacity(scope.lines.len());
        for line in &scope.lines {
            let content_json =
                line.content
                    .as_ref()
                    .ok_or_else(|| IntlError::UntranslatedLine {
                        scope_id: scope.id.clone(),
                        line_index: line.index,
                    })?;
            let content = convert_content_json(content_json)?;
            lines.push(LocaleLineEntry {
                content,
                audio_ref: line.audio.clone(),
            });
        }

        locale_tables.push(LocaleScopeTable { scope_id, lines });
    }

    let locale_data = LocaleData {
        locale_tag: locale_tag.to_string(),
        base_checksum: index.checksum,
        line_tables: locale_tables,
    };

    let mut buf = Vec::new();
    write_inkl(&locale_data, &mut buf);
    Ok(buf)
}

fn parse_scope_id(id_str: &str) -> Result<DefinitionId, IntlError> {
    // Format: "0x" + hex digits
    let hex = id_str
        .strip_prefix("0x")
        .ok_or_else(|| IntlError::InvalidScopeId(id_str.to_string()))?;
    let raw =
        u64::from_str_radix(hex, 16).map_err(|_| IntlError::InvalidScopeId(id_str.to_string()))?;
    DefinitionId::from_raw(raw).ok_or_else(|| IntlError::InvalidScopeId(id_str.to_string()))
}

fn convert_content_json(content: &ContentJson) -> Result<LineContent, IntlError> {
    match content {
        ContentJson::Plain(s) => Ok(LineContent::Plain(s.clone())),
        ContentJson::Template { template } => {
            let mut parts = Vec::with_capacity(template.len());
            for part in template {
                parts.push(convert_part_json(part)?);
            }
            Ok(LineContent::Template(parts))
        }
    }
}

fn convert_part_json(part: &PartJson) -> Result<LinePart, IntlError> {
    match part {
        PartJson::Literal(s) => Ok(LinePart::Literal(s.clone())),
        PartJson::Slot { slot } => Ok(LinePart::Slot(*slot)),
        PartJson::Select { select } => {
            let mut variants = Vec::with_capacity(select.variants.len());
            for map in &select.variants {
                for (key_str, val) in map {
                    let key = parse_select_key(key_str)?;
                    let text = val
                        .as_str()
                        .ok_or_else(|| IntlError::InvalidSelectKey(key_str.clone()))?;
                    variants.push((key, text.to_string()));
                }
            }
            Ok(LinePart::Select {
                slot: select.slot,
                variants,
                default: select.default.clone(),
            })
        }
    }
}

fn parse_select_key(key: &str) -> Result<SelectKey, IntlError> {
    if let Some(cat_str) = key.strip_prefix("cardinal:") {
        Ok(SelectKey::Cardinal(parse_plural_category(cat_str)?))
    } else if let Some(cat_str) = key.strip_prefix("ordinal:") {
        Ok(SelectKey::Ordinal(parse_plural_category(cat_str)?))
    } else if let Some(n_str) = key.strip_prefix('=') {
        let n = n_str
            .parse::<i32>()
            .map_err(|_| IntlError::InvalidSelectKey(key.to_string()))?;
        Ok(SelectKey::Exact(n))
    } else if let Some(kw) = key.strip_prefix("keyword:") {
        Ok(SelectKey::Keyword(kw.to_string()))
    } else {
        Err(IntlError::InvalidSelectKey(key.to_string()))
    }
}

fn parse_plural_category(s: &str) -> Result<PluralCategory, IntlError> {
    match s {
        "Zero" => Ok(PluralCategory::Zero),
        "One" => Ok(PluralCategory::One),
        "Two" => Ok(PluralCategory::Two),
        "Few" => Ok(PluralCategory::Few),
        "Many" => Ok(PluralCategory::Many),
        "Other" => Ok(PluralCategory::Other),
        _ => Err(IntlError::InvalidSelectKey(format!(
            "unknown plural category: {s}"
        ))),
    }
}
