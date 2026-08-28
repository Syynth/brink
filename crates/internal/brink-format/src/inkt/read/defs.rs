//! Defs/collections grammar-rule cluster: `.inkt` top-level definition
//! tables — the name table, globals, lists, struct shapes, list items,
//! externals, addresses, visibility/alias/frame-shape/effect-row tables, and
//! address paths / list literals.
//!
//! Pure `mod` extraction (issue #685) from the former monolithic `read.rs` —
//! no logic changes, only the module boundary is new.

use super::primitives::{
    err, next_rule, parse_def_id, parse_u8, parse_u16, parse_u32, unescape_string,
};
use super::values::{parse_value, parse_value_type};
use super::{InktParseError, P, Rule};
use crate::definition::{
    AddressDef, AddressPath, AliasEntry, CallAtom, CapabilityParam, DebugContainerTable,
    DebugEntry, DebugFileEntry, DebugInfoSection, DebugLocalEntry, DirectEffects, DispatchEntry,
    EffectRowEntry, ExternalFnDef, FileSurface, FrameShapeDef, GlobalVarDef, ListDef, ListItemDef,
    StructShapeDef,
};
use crate::id::{DefinitionId, NameId};
use crate::value::{ListValue, ShapeId};

// ── Name table ──────────────────────────────────────────────────────────────

pub(super) fn parse_name_table(pair: P<'_>) -> Result<Vec<String>, InktParseError> {
    let mut names = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::name_entry {
            let mut inner = entry.into_inner();
            let _index = inner.next(); // integer index (implied by position)
            let s = inner.next().ok_or_else(|| InktParseError {
                message: "expected string in name_entry".into(),
                line: 0,
                col: 0,
            })?;
            names.push(unescape_string(s.as_str()));
        }
    }
    Ok(names)
}

// ── Globals ─────────────────────────────────────────────────────────────────

pub(super) fn parse_globals(pair: P<'_>) -> Result<Vec<GlobalVarDef>, InktParseError> {
    let mut vars = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::global_entry {
            vars.push(parse_global_entry(entry)?);
        }
    }
    Ok(vars)
}

fn parse_global_entry(pair: P<'_>) -> Result<GlobalVarDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in global".into(),
        line: 0,
        col: 0,
    })?)?;

    let type_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected type_name in global".into(),
        line: 0,
        col: 0,
    })?;
    let value_type = parse_value_type(type_pair)?;

    let value_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected value in global".into(),
        line: 0,
        col: 0,
    })?;
    let default_value = parse_value(value_pair, Some(value_type))?;

    let mut mutable = false;
    let mut local = false;
    let mut name = NameId(0);

    for remaining in inner {
        match remaining.as_rule() {
            Rule::mutable_flag => mutable = true,
            Rule::local_flag => local = true,
            Rule::integer => {
                name = NameId(parse_u16(&remaining)?);
            }
            _ => {}
        }
    }

    Ok(GlobalVarDef {
        id,
        name,
        value_type,
        default_value,
        mutable,
        local,
    })
}

// ── Lists ───────────────────────────────────────────────────────────────────

pub(super) fn parse_lists(pair: P<'_>) -> Result<Vec<ListDef>, InktParseError> {
    let mut defs = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::list_entry {
            defs.push(parse_list_entry(entry)?);
        }
    }
    Ok(defs)
}

fn parse_list_entry(pair: P<'_>) -> Result<ListDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in list".into(),
        line: 0,
        col: 0,
    })?)?;

    let name_int = inner.next().ok_or_else(|| InktParseError {
        message: "expected name integer in list".into(),
        line: 0,
        col: 0,
    })?;
    let name = NameId(parse_u16(&name_int)?);

    let mut items = Vec::new();
    for remaining in inner {
        if remaining.as_rule() == Rule::list_item_inline {
            let mut li_inner = remaining.into_inner();
            let item_name_id = parse_u16(&li_inner.next().ok_or_else(|| InktParseError {
                message: "expected name in list item".into(),
                line: 0,
                col: 0,
            })?)?;
            let ordinal: i32 = li_inner
                .next()
                .ok_or_else(|| InktParseError {
                    message: "expected ordinal in list item".into(),
                    line: 0,
                    col: 0,
                })?
                .as_str()
                .parse()
                .map_err(|_| InktParseError {
                    message: "invalid ordinal".into(),
                    line: 0,
                    col: 0,
                })?;
            items.push((NameId(item_name_id), ordinal));
        }
    }

    Ok(ListDef { id, name, items })
}

// ── Struct shapes (TM-4, docs/format-v4-rfc.md §1) ───────────────────────────
// Mirrors the `.inkb` `StructShapes` section reader (the #742/#883 lesson —
// the writer and reader land together).

pub(super) fn parse_struct_shapes(pair: P<'_>) -> Result<Vec<StructShapeDef>, InktParseError> {
    let mut shapes = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::struct_shape_entry {
            shapes.push(parse_struct_shape_entry(entry)?);
        }
    }
    Ok(shapes)
}

fn parse_struct_shape_entry(pair: P<'_>) -> Result<StructShapeDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = ShapeId(
        inner
            .next()
            .ok_or_else(|| InktParseError {
                message: "expected shape id in struct".into(),
                line: 0,
                col: 0,
            })?
            .as_str()
            .parse()
            .map_err(|_| InktParseError {
                message: "invalid struct shape id".into(),
                line: 0,
                col: 0,
            })?,
    );

    let name_int = inner.next().ok_or_else(|| InktParseError {
        message: "expected name integer in struct".into(),
        line: 0,
        col: 0,
    })?;
    let name = NameId(parse_u16(&name_int)?);

    let mut fields = Vec::new();
    for remaining in inner {
        if remaining.as_rule() == Rule::struct_field {
            let field_int = remaining
                .into_inner()
                .next()
                .ok_or_else(|| InktParseError {
                    message: "expected name integer in struct field".into(),
                    line: 0,
                    col: 0,
                })?;
            fields.push(NameId(parse_u16(&field_int)?));
        }
    }

    Ok(StructShapeDef { id, name, fields })
}

// ── List items ──────────────────────────────────────────────────────────────

pub(super) fn parse_list_items(pair: P<'_>) -> Result<Vec<ListItemDef>, InktParseError> {
    let mut items = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::list_item_entry {
            items.push(parse_list_item_entry(entry)?);
        }
    }
    Ok(items)
}

fn parse_list_item_entry(pair: P<'_>) -> Result<ListItemDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(next_rule(&mut inner, Rule::def_id, "list_item id")?)?;
    let origin = parse_def_id(next_rule(&mut inner, Rule::def_id, "list_item origin")?)?;
    let ordinal: i32 = next_rule(&mut inner, Rule::integer, "list_item ordinal")?
        .as_str()
        .parse()
        .map_err(|_| InktParseError {
            message: "invalid ordinal".into(),
            line: 0,
            col: 0,
        })?;
    let name_val =
        next_rule(&mut inner, Rule::integer, "list_item name").map_or(Ok(0), |p| parse_u16(&p))?;
    Ok(ListItemDef {
        id,
        origin,
        ordinal,
        name: NameId(name_val),
    })
}

// ── Externals ───────────────────────────────────────────────────────────────

pub(super) fn parse_externals(pair: P<'_>) -> Result<Vec<ExternalFnDef>, InktParseError> {
    let mut exts = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::extern_entry {
            exts.push(parse_extern_entry(entry)?);
        }
    }
    Ok(exts)
}

fn parse_extern_entry(pair: P<'_>) -> Result<ExternalFnDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in extern".into(),
        line: 0,
        col: 0,
    })?)?;

    let argc_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected argc in extern".into(),
        line: 0,
        col: 0,
    })?;
    let arg_count: u8 = argc_pair
        .as_str()
        .parse()
        .map_err(|_| err(&argc_pair, "invalid argc"))?;

    let name_int = inner.next().ok_or_else(|| InktParseError {
        message: "expected name in extern".into(),
        line: 0,
        col: 0,
    })?;
    let name = NameId(parse_u16(&name_int)?);

    let mut fallback = None;
    for remaining in inner {
        if remaining.as_rule() == Rule::fallback {
            let fb_inner = remaining
                .into_inner()
                .next()
                .ok_or_else(|| InktParseError {
                    message: "expected def_id in fallback".into(),
                    line: 0,
                    col: 0,
                })?;
            fallback = Some(parse_def_id(fb_inner)?);
        }
    }

    Ok(ExternalFnDef {
        id,
        name,
        arg_count,
        fallback,
    })
}

// ── Addresses ───────────────────────────────────────────────────────────────

pub(super) fn parse_addresses(pair: P<'_>) -> Result<Vec<AddressDef>, InktParseError> {
    let mut addresses = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::address_entry {
            addresses.push(parse_address_entry(entry)?);
        }
    }
    Ok(addresses)
}

fn parse_address_entry(pair: P<'_>) -> Result<AddressDef, InktParseError> {
    let mut inner = pair.into_inner();
    let id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected def_id in address".into(),
        line: 0,
        col: 0,
    })?)?;
    let container_id = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected container_id in address".into(),
        line: 0,
        col: 0,
    })?)?;
    let offset_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected byte_offset in address".into(),
        line: 0,
        col: 0,
    })?;
    let byte_offset: u32 = offset_pair
        .as_str()
        .parse()
        .map_err(|_| err(&offset_pair, "invalid byte_offset"))?;
    Ok(AddressDef {
        id,
        container_id,
        byte_offset,
    })
}

pub(super) fn parse_visibility(pair: P<'_>) -> Result<Vec<DefinitionId>, InktParseError> {
    let mut ids = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::private_entry {
            let id_pair = entry.into_inner().next().ok_or_else(|| InktParseError {
                message: "expected def_id in private entry".into(),
                line: 1,
                col: 1,
            })?;
            ids.push(parse_def_id(id_pair)?);
        }
    }
    Ok(ids)
}

/// M-3 (`docs/modules-spec.md` §5): parse `(alias_table (alias $old -> $new) …)`.
pub(super) fn parse_alias_table(pair: P<'_>) -> Result<Vec<AliasEntry>, InktParseError> {
    let mut aliases = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::alias_entry {
            aliases.push(parse_alias_entry(entry)?);
        }
    }
    Ok(aliases)
}

fn parse_alias_entry(pair: P<'_>) -> Result<AliasEntry, InktParseError> {
    let mut inner = pair.into_inner();
    let old = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected old def_id in alias".into(),
        line: 0,
        col: 0,
    })?)?;
    let new = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected new def_id in alias".into(),
        line: 0,
        col: 0,
    })?)?;
    Ok(AliasEntry { old, new })
}

/// FS-3 (`docs/flow-suspension-spec.md` §4/§11): parse
/// `(frame_shapes (frame $site $slot …) …)`. Each `frame` entry is the
/// `await` site's stable `DefinitionId` followed by its name-keyed
/// crossing-local slots (interned `NameId`s).
pub(super) fn parse_frame_shapes(pair: P<'_>) -> Result<Vec<FrameShapeDef>, InktParseError> {
    let mut shapes = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::frame_shape_entry {
            shapes.push(parse_frame_shape_entry(entry)?);
        }
    }
    Ok(shapes)
}

fn parse_frame_shape_entry(pair: P<'_>) -> Result<FrameShapeDef, InktParseError> {
    let mut inner = pair.into_inner();
    let site = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected site def_id in frame shape".into(),
        line: 0,
        col: 0,
    })?)?;
    let mut slots = Vec::new();
    for slot in inner {
        slots.push(NameId(parse_u16(&slot)?));
    }
    Ok(FrameShapeDef { site, slots })
}

/// D6 (`docs/debugger-spec.md` §2): parse
/// `(debug_info (files …)? (dcontainer $idx (entry …)* (locals …)?)* )`.
/// `debug_info` is only present in the text at all when debug info was
/// requested (mirroring [`crate::StoryData::debug_info`]'s `Option`), so
/// the caller in `parse_story` sets `Some` unconditionally on a match —
/// there is no empty-but-present case for this rule to produce.
pub(super) fn parse_debug_info(pair: P<'_>) -> Result<DebugInfoSection, InktParseError> {
    let mut files = Vec::new();
    let mut containers = Vec::new();
    for section in pair.into_inner() {
        match section.as_rule() {
            Rule::debug_files => {
                for entry in section.into_inner() {
                    if entry.as_rule() == Rule::debug_file_entry {
                        files.push(parse_debug_file_entry(entry)?);
                    }
                }
            }
            Rule::debug_container => containers.push(parse_debug_container(section)?),
            _ => {}
        }
    }
    Ok(DebugInfoSection { files, containers })
}

/// `(file $idx synthetic|ink|native "path")`. The leading index is
/// positional bookkeeping the writer emits for readability — the reader
/// trusts encounter order (matching the writer's own `enumerate()`), not
/// this integer, exactly like [`parse_debug_container`] below.
fn parse_debug_file_entry(pair: P<'_>) -> Result<DebugFileEntry, InktParseError> {
    let mut inner = pair.into_inner();
    let _idx = inner.next().ok_or_else(|| InktParseError {
        message: "expected file index".into(),
        line: 0,
        col: 0,
    })?;
    let surface_pair = next_rule(&mut inner, Rule::debug_surface, "debug file entry")?;
    let surface = match surface_pair.as_str() {
        "synthetic" => FileSurface::Synthetic,
        "ink" => FileSurface::Ink,
        "native" => FileSurface::Native,
        other => return Err(err(&surface_pair, format!("unknown surface: {other}"))),
    };
    let path_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected path string in file entry".into(),
        line: 0,
        col: 0,
    })?;
    Ok(DebugFileEntry {
        surface,
        path: unescape_string(path_pair.as_str()),
    })
}

/// `(dcontainer $idx (entry …)* (locals …)?)`. The leading index is
/// positional bookkeeping the writer emits (matching
/// `debug_info.containers`' own position, which is `Containers`-lockstep) —
/// the reader trusts encounter order, not this integer, so a
/// hand-edited `.inkt` with a misleading index still round-trips on
/// structure; only a genuinely reordered/missing block changes behavior.
fn parse_debug_container(pair: P<'_>) -> Result<DebugContainerTable, InktParseError> {
    let mut entries = Vec::new();
    let mut locals = Vec::new();
    for inner in pair.into_inner() {
        match inner.as_rule() {
            Rule::integer => {}
            Rule::debug_entry => entries.push(parse_debug_entry(inner)?),
            Rule::debug_locals => {
                for local in inner.into_inner() {
                    if local.as_rule() == Rule::debug_local_entry {
                        locals.push(parse_debug_local_entry(local)?);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(DebugContainerTable { entries, locals })
}

/// `(entry $offset $file_idx $range_start $range_len $kind_token $flags)`.
fn parse_debug_entry(pair: P<'_>) -> Result<DebugEntry, InktParseError> {
    let mut inner = pair.into_inner();
    let mut next_u32 = |ctx: &str| -> Result<u32, InktParseError> {
        let p = inner.next().ok_or_else(|| InktParseError {
            message: format!("expected {ctx} in debug entry"),
            line: 0,
            col: 0,
        })?;
        parse_u32(&p)
    };
    let bytecode_offset = next_u32("bytecode_offset")?;
    let file_idx = next_u32("file_idx")?;
    let range_start = next_u32("range_start")?;
    let range_len = next_u32("range_len")?;
    let kind_token = next_u32("kind_token")?;
    let flags_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected flags in debug entry".into(),
        line: 0,
        col: 0,
    })?;
    let flags = parse_u8(&flags_pair)?;
    Ok(DebugEntry {
        bytecode_offset,
        file_idx,
        range_start,
        range_len,
        kind_token,
        flags,
    })
}

/// `(local $slot "name" (range $file_idx $start $len)?)`.
fn parse_debug_local_entry(pair: P<'_>) -> Result<DebugLocalEntry, InktParseError> {
    let mut inner = pair.into_inner();
    let slot_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected slot in local entry".into(),
        line: 0,
        col: 0,
    })?;
    let slot = parse_u16(&slot_pair)?;
    let name_pair = inner.next().ok_or_else(|| InktParseError {
        message: "expected name in local entry".into(),
        line: 0,
        col: 0,
    })?;
    let name = unescape_string(name_pair.as_str());
    let declaring_range = match inner.next() {
        Some(range_pair) if range_pair.as_rule() == Rule::debug_range => {
            let mut r = range_pair.into_inner();
            let mut next_u32 = |ctx: &str| -> Result<u32, InktParseError> {
                let p = r.next().ok_or_else(|| InktParseError {
                    message: format!("expected {ctx} in local declaring range"),
                    line: 0,
                    col: 0,
                })?;
                parse_u32(&p)
            };
            let file_idx = next_u32("file_idx")?;
            let range_start = next_u32("range_start")?;
            let range_len = next_u32("range_len")?;
            Some((file_idx, range_start, range_len))
        }
        _ => None,
    };
    Ok(DebugLocalEntry {
        slot,
        name,
        declaring_range,
    })
}

/// T2-3 (`docs/effects-spec.md` §11): parse `(effect_rows (row …) …)`.
pub(super) fn parse_effect_rows(pair: P<'_>) -> Result<Vec<EffectRowEntry>, InktParseError> {
    let mut rows = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::effect_row {
            rows.push(parse_effect_row(&entry)?);
        }
    }
    Ok(rows)
}

fn parse_effect_row(pair: &P<'_>) -> Result<EffectRowEntry, InktParseError> {
    let mut inner = pair.clone().into_inner();
    let def = parse_def_id(
        inner
            .next()
            .ok_or_else(|| err(pair, "expected row def_id"))?,
    )?;
    // #882 freeze bit: defaults to `true` (host entry point) — `internal_flag`
    // is present only for a `#@private` def (see `EffectRowEntry::is_entry`'s
    // doc). Parsed rule-by-rule (not positionally) because this optional
    // token sits before the mandatory reads/writes/calls triple.
    let mut is_entry = true;
    let mut reads = None;
    let mut writes = None;
    let mut calls = None;
    let mut opaque = false;
    let mut emits = false;
    let mut tags = false;
    let mut faults = false;
    let mut dispatches = Vec::new();
    for rest in inner {
        match rest.as_rule() {
            Rule::internal_flag => is_entry = false,
            Rule::effects_reads => reads = Some(parse_effect_cells(rest)?),
            Rule::effects_writes => writes = Some(parse_effect_cells(rest)?),
            Rule::effects_calls => calls = Some(parse_effect_calls(rest)?),
            Rule::opaque_flag => opaque = true,
            Rule::emits_flag => emits = true,
            Rule::tags_flag => tags = true,
            Rule::faults_flag => faults = true,
            Rule::dispatch_entry => dispatches.push(parse_dispatch_entry(&rest)?),
            _ => {}
        }
    }
    let reads = reads.ok_or_else(|| err(pair, "expected reads"))?;
    let writes = writes.ok_or_else(|| err(pair, "expected writes"))?;
    let calls = calls.ok_or_else(|| err(pair, "expected calls"))?;
    Ok(EffectRowEntry {
        def,
        is_entry,
        direct: DirectEffects {
            reads,
            writes,
            calls,
            opaque,
            emits,
            tags,
            faults,
        },
        dispatches,
    })
}

fn parse_dispatch_entry(pair: &P<'_>) -> Result<DispatchEntry, InktParseError> {
    let mut cell = None;
    let mut narrowable = false;
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut calls = Vec::new();
    let mut opaque = false;
    let mut emits = false;
    let mut tags = false;
    let mut faults = false;
    for part in pair.clone().into_inner() {
        match part.as_rule() {
            Rule::def_id => cell = Some(parse_def_id(part)?),
            Rule::narrowable_flag => narrowable = true,
            Rule::effects_reads => reads = parse_effect_cells(part)?,
            Rule::effects_writes => writes = parse_effect_cells(part)?,
            Rule::effects_calls => calls = parse_effect_calls(part)?,
            Rule::opaque_flag => opaque = true,
            Rule::emits_flag => emits = true,
            Rule::tags_flag => tags = true,
            Rule::faults_flag => faults = true,
            _ => {}
        }
    }
    let cell = cell.ok_or_else(|| err(pair, "expected dispatch cell def_id"))?;
    Ok(DispatchEntry {
        cell,
        narrowable,
        fallback: DirectEffects {
            reads,
            writes,
            calls,
            opaque,
            emits,
            tags,
            faults,
        },
    })
}

/// Parse a `(reads …)` / `(writes …)` cell list — a run of `def_id`s.
fn parse_effect_cells(pair: P<'_>) -> Result<Vec<DefinitionId>, InktParseError> {
    let mut cells = Vec::new();
    for id in pair.into_inner() {
        if id.as_rule() == Rule::def_id {
            cells.push(parse_def_id(id)?);
        }
    }
    Ok(cells)
}

/// Parse a `(calls (call <name> any) …)` atom list.
fn parse_effect_calls(pair: P<'_>) -> Result<Vec<CallAtom>, InktParseError> {
    let mut calls = Vec::new();
    for atom in pair.into_inner() {
        if atom.as_rule() == Rule::call_atom {
            calls.push(parse_call_atom(&atom)?);
        }
    }
    Ok(calls)
}

fn parse_call_atom(pair: &P<'_>) -> Result<CallAtom, InktParseError> {
    let mut inner = pair.clone().into_inner();
    let name_pair = inner
        .next()
        .ok_or_else(|| err(pair, "expected call atom name"))?;
    let name = NameId(parse_u16(&name_pair)?);
    // The capability-parameter slot: `any` is the only v1 value (the grammar's
    // `cap_param` rule accepts only that literal). The reserved handle-parameter
    // slot is `None` in v1 — nothing textual carries a bound handle.
    let capability = CapabilityParam::Any;
    Ok(CallAtom {
        name,
        capability,
        handle_param: None,
    })
}

pub(super) fn parse_address_paths(pair: P<'_>) -> Result<Vec<AddressPath>, InktParseError> {
    let mut paths = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::address_path_entry {
            paths.push(parse_address_path_entry(entry)?);
        }
    }
    Ok(paths)
}

fn parse_address_path_entry(pair: P<'_>) -> Result<AddressPath, InktParseError> {
    let mut inner = pair.into_inner();
    let path_int = inner.next().ok_or_else(|| InktParseError {
        message: "expected path index in address_path".into(),
        line: 0,
        col: 0,
    })?;
    let path = NameId(parse_u16(&path_int)?);
    let target = parse_def_id(inner.next().ok_or_else(|| InktParseError {
        message: "expected target def_id in address_path".into(),
        line: 0,
        col: 0,
    })?)?;
    Ok(AddressPath { path, target })
}

// ── List literals ────────────────────────────────────────────────────────────

pub(super) fn parse_list_literals(pair: P<'_>) -> Result<Vec<ListValue>, InktParseError> {
    let mut literals = Vec::new();
    for entry in pair.into_inner() {
        if entry.as_rule() == Rule::list_literal_entry {
            literals.push(parse_list_literal_entry(entry)?);
        }
    }
    Ok(literals)
}

fn parse_list_literal_entry(pair: P<'_>) -> Result<ListValue, InktParseError> {
    let mut items = Vec::new();
    let mut origins = Vec::new();

    for child in pair.into_inner() {
        match child.as_rule() {
            Rule::list_value_items => {
                for def_pair in child.into_inner() {
                    if def_pair.as_rule() == Rule::def_id {
                        items.push(parse_def_id(def_pair)?);
                    }
                }
            }
            Rule::list_value_origins => {
                for def_pair in child.into_inner() {
                    if def_pair.as_rule() == Rule::def_id {
                        origins.push(parse_def_id(def_pair)?);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(ListValue { items, origins })
}
