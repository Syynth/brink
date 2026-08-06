//! Compile translated `lines.json` into a `.inkl` locale overlay.

use std::collections::{BTreeMap, HashMap};

use brink_format::{
    DefinitionId, LineContent, LinePart, LocaleData, LocaleLineEntry, LocaleScopeTable,
    PluralCategory, ScopeLineTable, SelectKey, read_inkb_index, read_section_alias_table,
    read_section_line_tables, write_inkl,
};

use crate::error::IntlError;
use crate::json_model::{ContentJson, LinesJson, PartJson};
use crate::scope_alias::{ScopeAliasIndex, format_scope_id, parse_scope_id_lenient};

/// Compile a translated `LinesJson` against a base `.inkb` file into `.inkl` bytes.
///
/// `base_inkb` is the raw `.inkb` bytes. `lines_json` is the deserialized
/// translated lines. `locale_tag` is a BCP 47 locale string (e.g. "es", "ja").
///
/// # Rename rebinding (#1442)
///
/// A translation exported before a declared `#@was` rename carries the
/// definition's *pre-rename* scope id, which no longer appears in the base
/// `.inkb`. Rather than failing with [`IntlError::ScopeNotInBase`], such a
/// scope is rebound through the base file's own `AliasTable` section — the
/// same `#@was`-derived edges the save path consults. Rebinding is automatic
/// and needs no separate CLI migration step: the alias edges already ride in
/// the artifact this function reads, and a rename can happen on any recompile,
/// so a manual step would fail open every time an author forgot to run it.
/// (`migrate_unit_ids` / `brink migrate-xliff` remains the tool for the one-off
/// *id-spelling* migration of PR #1594 — a format change, not an id move.)
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
    let aliases = ScopeAliasIndex::new(&read_section_alias_table(base_inkb, &index)?);

    // Build lookup from scope_id → base scope line table (line count +
    // per-line slot metadata).
    let base_scope_map: HashMap<DefinitionId, &ScopeLineTable> =
        base_tables.iter().map(|lt| (lt.scope_id, lt)).collect();

    let bindings = resolve_scope_bindings(lines_json, &base_scope_map, &aliases)?;

    let mut locale_tables = Vec::with_capacity(lines_json.scopes.len());

    for (scope, binding) in lines_json.scopes.iter().zip(&bindings) {
        let scope_id = binding.scope_id;

        let base_table = base_scope_map
            .get(&scope_id)
            .copied()
            .ok_or_else(|| IntlError::ScopeNotInBase(scope.id.clone()))?;
        let base_line_count = base_table.lines.len();

        if scope.lines.len() != base_line_count {
            return Err(IntlError::LineCountMismatch {
                scope_id: scope.id.clone(),
                expected: base_line_count,
                actual: scope.lines.len(),
            });
        }

        let mut lines = Vec::with_capacity(scope.lines.len());
        for (pos, line) in scope.lines.iter().enumerate() {
            let content_json =
                line.content
                    .as_ref()
                    .ok_or_else(|| IntlError::UntranslatedLine {
                        scope_id: scope.id.clone(),
                        line_index: line.index,
                    })?;

            // Structural re-import validation (#1445): a translated line's
            // template must only reference slot indices that exist in the
            // *base* line at this position — the base `.inkb` is the sole
            // source of truth for how many values `EmitLine` pushes at
            // runtime. An out-of-range index would otherwise compile
            // silently and resolve to empty/default text at playback
            // (`resolve_line_ref` / `resolve_select` both fall back
            // silently on a missing slot) instead of failing at
            // compile-locale time.
            let base_slot_count = base_table.lines[pos].slot_info.len();
            validate_slot_indices(content_json, base_slot_count, &scope.id, line.index)?;

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

/// The base scope a translated scope binds to, and whether getting there
/// needed a `#@was` alias edge.
struct ScopeBinding {
    scope_id: DefinitionId,
    rebound: bool,
}

/// Resolve every translated scope to the base scope it compiles against,
/// following `#@was` alias edges for ids the base no longer knows.
///
/// A direct match always wins over an alias edge, so a translation that was
/// already regenerated past the rename is unaffected. Two translated scopes
/// landing on the same base scope is rejected rather than silently letting
/// the last one win (that would be a silent data drop) — but only when a
/// rebind is involved, leaving the pre-existing duplicate-id behavior alone.
fn resolve_scope_bindings(
    lines_json: &LinesJson,
    base_scope_map: &HashMap<DefinitionId, &ScopeLineTable>,
    aliases: &ScopeAliasIndex,
) -> Result<Vec<ScopeBinding>, IntlError> {
    let mut bindings = Vec::with_capacity(lines_json.scopes.len());
    for scope in &lines_json.scopes {
        let declared = parse_scope_id(&scope.id)?;
        let binding = if base_scope_map.contains_key(&declared) {
            ScopeBinding {
                scope_id: declared,
                rebound: false,
            }
        } else {
            match aliases.current(declared) {
                Some(current) if base_scope_map.contains_key(&current) => ScopeBinding {
                    scope_id: current,
                    rebound: true,
                },
                // No usable alias edge: keep the declared id so the caller
                // reports `ScopeNotInBase` naming the id the file carried.
                _ => ScopeBinding {
                    scope_id: declared,
                    rebound: false,
                },
            }
        };
        bindings.push(binding);
    }

    let mut claims: BTreeMap<DefinitionId, Vec<usize>> = BTreeMap::new();
    for (idx, binding) in bindings.iter().enumerate() {
        claims.entry(binding.scope_id).or_default().push(idx);
    }
    for (base_scope_id, claimants) in &claims {
        if claimants.len() > 1 && claimants.iter().any(|&i| bindings[i].rebound) {
            return Err(IntlError::AmbiguousScopeRebind {
                scope_ids: claimants
                    .iter()
                    .map(|&i| lines_json.scopes[i].id.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
                base_scope_id: format_scope_id(*base_scope_id),
            });
        }
    }

    Ok(bindings)
}

/// Validate that every slot index referenced by a translated line's
/// template — both plain `{slot n}` interpolations and `Select` branches —
/// exists in the base line at this position.
///
/// Plain (non-template) content has no slots and trivially passes. Literal
/// parts carry no index and are skipped.
fn validate_slot_indices(
    content: &ContentJson,
    base_slot_count: usize,
    scope_id: &str,
    line_index: u16,
) -> Result<(), IntlError> {
    let ContentJson::Template { template } = content else {
        return Ok(());
    };

    validate_slot_indices_in_parts(template, base_slot_count, scope_id, line_index)
}

/// [`validate_slot_indices`]'s per-part check, recursing into a
/// [`PartJson::Span`]'s `children` — a span can carry a real interpolation
/// slot (`<b>{name}</b>`), so a mangled index inside one must be caught
/// exactly like a top-level one is; skipping span children here would let
/// a corrupted translation through with a silently-ignored bad slot
/// reference.
fn validate_slot_indices_in_parts(
    parts: &[PartJson],
    base_slot_count: usize,
    scope_id: &str,
    line_index: u16,
) -> Result<(), IntlError> {
    for part in parts {
        let slot = match part {
            PartJson::Slot { slot } => *slot,
            PartJson::Select { select } => select.slot,
            PartJson::Span { span } => {
                validate_slot_indices_in_parts(
                    &span.children,
                    base_slot_count,
                    scope_id,
                    line_index,
                )?;
                continue;
            }
            PartJson::Literal(_) => continue,
        };
        if slot as usize >= base_slot_count {
            return Err(IntlError::SlotIndexOutOfRange {
                scope_id: scope_id.to_string(),
                line_index,
                slot,
                slot_count: base_slot_count,
            });
        }
    }
    Ok(())
}

fn parse_scope_id(id_str: &str) -> Result<DefinitionId, IntlError> {
    // Format: "0x" + hex digits. Locale compilation is the strict side of the
    // workflow — an unparseable scope id is a hard error here, while
    // regeneration merely skips rebinding for it.
    parse_scope_id_lenient(id_str).ok_or_else(|| IntlError::InvalidScopeId(id_str.to_string()))
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
        PartJson::Span { span } => {
            let mut children = Vec::with_capacity(span.children.len());
            for child in &span.children {
                children.push(convert_part_json(child)?);
            }
            Ok(LinePart::Span {
                name: span.name.clone(),
                attrs: span
                    .attrs
                    .iter()
                    .map(|a| (a.name.clone(), a.value.clone()))
                    .collect(),
                children,
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
