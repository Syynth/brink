//! Containers/lines/i18n grammar-rule cluster: `.inkt` container metadata
//! (flags, path hash, params) plus the per-scope line table — plain lines,
//! ICU-style templates, select/plural variants.
//!
//! Pure `mod` extraction (issue #685) from the former monolithic `read.rs` —
//! no logic changes, only the module boundary is new.

use super::instructions::parse_code_field;
use super::primitives::{err, parse_def_id, parse_hex_u64, parse_u16, unescape_string};
use super::{InktParseError, P, Rule};
use crate::counting::CountingFlags;
use crate::definition::{
    ContainerDef, LineEntry, ParamMeta, ScopeLineTable, SlotInfo, SourceLocation,
};
use crate::id::NameId;
use crate::line::{LineContent, LinePart, PluralCategory, SelectKey};

// ── Containers ──────────────────────────────────────────────────────────────

#[expect(
    clippy::too_many_lines,
    reason = "one-arm-per-field container parser; splitting would scatter the field table"
)]
pub(super) fn parse_container(
    pair: P<'_>,
) -> Result<(ContainerDef, ScopeLineTable), InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in container".into(),
        line: 0,
        col: 0,
    })?)?;

    let mut counting_flags = CountingFlags::empty();
    let mut path_hash = 0i32;
    let mut param_count = 0u8;
    let mut params: Vec<ParamMeta> = Vec::new();
    let mut local = false;
    let mut lines = Vec::new();
    let mut bytecode = Vec::new();
    let mut name: Option<NameId> = None;

    let mut scope_id = id;

    for child in inner {
        match child.as_rule() {
            Rule::scope_field => {
                let scope_pair = child.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected def_id in scope".into(),
                    line: 0,
                    col: 0,
                })?;
                scope_id = parse_def_id(scope_pair)?;
            }
            Rule::container_name_field => {
                let val = child.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected integer in container name".into(),
                    line: 0,
                    col: 0,
                })?;
                name = Some(NameId(parse_u16(&val)?));
            }
            Rule::flags_field => {
                for flag in child.into_inner() {
                    if flag.as_rule() == Rule::flag_name {
                        match flag.as_str() {
                            "visits" => counting_flags |= CountingFlags::VISITS,
                            "turns" => counting_flags |= CountingFlags::TURNS,
                            "start_only" => counting_flags |= CountingFlags::COUNT_START_ONLY,
                            "invisible" => counting_flags |= CountingFlags::INVISIBLE,
                            _ => {}
                        }
                    }
                }
            }
            Rule::path_hash_field => {
                let val = child.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected integer in path_hash".into(),
                    line: 0,
                    col: 0,
                })?;
                path_hash = val.as_str().parse().map_err(|_| InktParseError {
                    message: "invalid path_hash integer".into(),
                    line: 0,
                    col: 0,
                })?;
            }
            Rule::params_field => {
                let mut fields = child.into_inner();
                let val = fields.next().ok_or_else(|| InktParseError {
                    message: "expected integer in params".into(),
                    line: 0,
                    col: 0,
                })?;
                param_count = val.as_str().parse().map_err(|_| InktParseError {
                    message: "invalid params integer".into(),
                    line: 0,
                    col: 0,
                })?;
                // Per-param name/mode metadata (T1c, #700): `(val id)` / `(ref
                // id)` entries after the count, in declared order.
                for meta in fields {
                    if meta.as_rule() != Rule::param_meta {
                        continue;
                    }
                    let mut mi = meta.into_inner();
                    let mode = mi.next().ok_or_else(|| InktParseError {
                        message: "expected mode in param_meta".into(),
                        line: 0,
                        col: 0,
                    })?;
                    let is_ref = mode.as_str() == "ref";
                    let name_int = mi.next().ok_or_else(|| InktParseError {
                        message: "expected name id in param_meta".into(),
                        line: 0,
                        col: 0,
                    })?;
                    params.push(ParamMeta {
                        name: NameId(parse_u16(&name_int)?),
                        is_ref,
                    });
                }
                // `ContainerDef::params`'s doc invariant: `params.len()`
                // always equals `param_count` whenever per-param metadata is
                // present at all (empty `params` is the separate, legitimate
                // "count only, no metadata" case — e.g. the converter
                // pipeline). A `.inkt` file asserting otherwise (fuzz-found,
                // #745) is malformed input, not silently-acceptable data:
                // `write_inkt`'s `(params N …)` clause is gated on
                // `param_count != 0`, so an inconsistent `param_count: 0` with
                // non-empty `params` would round-trip by silently dropping
                // the params entirely on the next write.
                if !params.is_empty() && params.len() != usize::from(param_count) {
                    return Err(InktParseError {
                        message: format!(
                            "params metadata count ({}) does not match declared param_count ({param_count})",
                            params.len()
                        ),
                        line: 0,
                        col: 0,
                    });
                }
            }
            Rule::local_flag => local = true,
            Rule::lines_field => {
                lines = parse_lines_field(child)?;
            }
            Rule::code_field => {
                bytecode = parse_code_field(child)?;
            }
            _ => {}
        }
    }

    let container = ContainerDef {
        id,
        scope_id,
        name,
        bytecode,
        counting_flags,
        path_hash,
        param_count,
        // Per-param name/mode metadata (T1c, #700), reconstructed from the
        // `(params N (mode id)…)` dump so the `.inkt` round-trip is lossless
        // (matches the binary `.inkb` path used by persistence/rehydration).
        params,
        local,
    };
    let line_table = ScopeLineTable { scope_id, lines };
    Ok((container, line_table))
}

fn parse_lines_field(pair: P<'_>) -> Result<Vec<LineEntry>, InktParseError> {
    let mut entries = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::line_entry {
            entries.push(parse_line_entry(entry)?);
        }
    }
    Ok(entries)
}

fn parse_line_entry(pair: P<'_>) -> Result<LineEntry, InktParseError> {
    let mut inner = pair.into_inner();
    let _index = inner.next(); // integer index (implied by position)
    let content_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected line content".into(),
        line: 0,
        col: 0,
    })?;
    let content = parse_line_content(content_pair)?;
    let hash_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected source_hash".into(),
        line: 0,
        col: 0,
    })?;
    // source_hash is @HHHHHHHHHHHHHHHH
    let hash_str = hash_pair.as_str();
    let source_hash = parse_hex_u64(&format!("0x{}", &hash_str[1..]))?;

    let mut audio_ref = None;
    let mut slot_info = Vec::new();
    let mut source_location = None;

    for remaining in inner {
        match remaining.as_rule() {
            Rule::audio_field => {
                let s = remaining
                    .into_inner()
                    .next()
                    .ok_or_else(|| InktParseError {
                        message: "expected audio string".into(),
                        line: 0,
                        col: 0,
                    })?;
                audio_ref = Some(unescape_string(s.as_str()));
            }
            Rule::slots_field => {
                for slot_entry in remaining.into_inner() {
                    if slot_entry.as_rule() == Rule::slot_entry {
                        let mut parts = slot_entry.into_inner();
                        let idx_str = parts.next().map_or("0", |p| p.as_str());
                        let idx: u8 = idx_str.parse().unwrap_or(0);
                        let name_str = parts
                            .next()
                            .map_or_else(String::new, |p| unescape_string(p.as_str()));
                        slot_info.push(SlotInfo {
                            index: idx,
                            name: name_str,
                        });
                    }
                }
            }
            Rule::source_field => {
                let mut parts = remaining.into_inner();
                let file = parts
                    .next()
                    .map_or_else(String::new, |p| unescape_string(p.as_str()));
                let start: u32 = parts
                    .next()
                    .and_then(|p| p.as_str().parse().ok())
                    .unwrap_or(0);
                let end: u32 = parts
                    .next()
                    .and_then(|p| p.as_str().parse().ok())
                    .unwrap_or(0);
                source_location = Some(SourceLocation {
                    file,
                    range_start: start,
                    range_end: end,
                });
            }
            _ => {}
        }
    }

    let flags = crate::LineFlags::from_content(&content);
    Ok(LineEntry {
        content,
        flags,
        source_hash,
        audio_ref,
        slot_info,
        source_location,
    })
}

fn parse_line_content(pair: P<'_>) -> Result<LineContent, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty line content".into(),
        line: 0,
        col: 0,
    })?;
    match inner.as_rule() {
        Rule::string => Ok(LineContent::Plain(unescape_string(inner.as_str()))),
        Rule::template => parse_template(inner),
        _ => Err(err(
            &inner,
            format!("unexpected line content: {:?}", inner.as_rule()),
        )),
    }
}

fn parse_template(pair: P<'_>) -> Result<LineContent, InktParseError> {
    let mut parts = Vec::new();
    for child in pair.into_inner() {
        if child.as_rule() == Rule::template_part {
            parts.push(parse_template_part(child)?);
        }
    }
    Ok(LineContent::Template(parts))
}

fn parse_template_part(pair: P<'_>) -> Result<LinePart, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty template part".into(),
        line: 0,
        col: 0,
    })?;
    match inner.as_rule() {
        Rule::literal_part => {
            let s = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected string in literal".into(),
                line: 0,
                col: 0,
            })?;
            Ok(LinePart::Literal(unescape_string(s.as_str())))
        }
        Rule::slot_part => {
            let idx = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected integer in slot".into(),
                line: 0,
                col: 0,
            })?;
            let n: u8 = idx
                .as_str()
                .parse()
                .map_err(|_| err(&idx, "invalid slot index"))?;
            Ok(LinePart::Slot(n))
        }
        Rule::select_part => parse_select_part(inner),
        _ => Err(err(
            &inner,
            format!("unexpected template part: {:?}", inner.as_rule()),
        )),
    }
}

fn parse_select_part(pair: P<'_>) -> Result<LinePart, InktParseError> {
    let mut inner = pair.into_inner();
    let slot_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected slot in select".into(),
        line: 0,
        col: 0,
    })?;
    let slot: u8 = slot_pair
        .as_str()
        .parse()
        .map_err(|_| err(&slot_pair, "invalid slot"))?;

    let mut variants = Vec::new();
    let mut default = String::new();

    for child in inner {
        match child.as_rule() {
            Rule::select_variant => {
                let mut vi = child.into_inner();
                let key_pair = vi.next().ok_or_else(|| InktParseError {
                    message: "expected key in variant".into(),
                    line: 0,
                    col: 0,
                })?;
                let key = parse_select_key(key_pair)?;
                let text = vi.next().ok_or_else(|| InktParseError {
                    message: "expected text in variant".into(),
                    line: 0,
                    col: 0,
                })?;
                variants.push((key, unescape_string(text.as_str())));
            }
            Rule::select_default => {
                let s = child.into_inner().next().ok_or_else(|| InktParseError {
                    message: "expected string in default".into(),
                    line: 0,
                    col: 0,
                })?;
                default = unescape_string(s.as_str());
            }
            _ => {}
        }
    }

    Ok(LinePart::Select {
        slot,
        variants,
        default,
    })
}

fn parse_select_key(pair: P<'_>) -> Result<SelectKey, InktParseError> {
    let inner = pair.into_inner().next().ok_or_else(|| InktParseError {
        message: "empty select key".into(),
        line: 0,
        col: 0,
    })?;
    match inner.as_rule() {
        Rule::cardinal_key => {
            let cat = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected plural_cat".into(),
                line: 0,
                col: 0,
            })?;
            Ok(SelectKey::Cardinal(parse_plural_cat(cat)?))
        }
        Rule::ordinal_key => {
            let cat = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected plural_cat".into(),
                line: 0,
                col: 0,
            })?;
            Ok(SelectKey::Ordinal(parse_plural_cat(cat)?))
        }
        Rule::exact_key => {
            let n = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected integer".into(),
                line: 0,
                col: 0,
            })?;
            let v: i32 = n
                .as_str()
                .parse()
                .map_err(|_| err(&n, "invalid exact key"))?;
            Ok(SelectKey::Exact(v))
        }
        Rule::keyword_key => {
            let ident = inner.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected ident".into(),
                line: 0,
                col: 0,
            })?;
            Ok(SelectKey::Keyword(ident.as_str().to_owned()))
        }
        _ => Err(err(
            &inner,
            format!("unexpected select key: {:?}", inner.as_rule()),
        )),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn parse_plural_cat(pair: P<'_>) -> Result<PluralCategory, InktParseError> {
    match pair.as_str() {
        "Zero" => Ok(PluralCategory::Zero),
        "One" => Ok(PluralCategory::One),
        "Two" => Ok(PluralCategory::Two),
        "Few" => Ok(PluralCategory::Few),
        "Many" => Ok(PluralCategory::Many),
        "Other" => Ok(PluralCategory::Other),
        _ => Err(err(
            &pair,
            format!("unknown plural category: {}", pair.as_str()),
        )),
    }
}
