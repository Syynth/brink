//! Encoding (write) half of the `.inkb` binary format.

use alloc::string::String;
use alloc::vec::Vec;

use crate::codec::{
    crc32, write_def_id, write_i32, write_str, write_u8, write_u16, write_u32, write_u64,
};
use crate::definition::{
    AddressDef, AddressPath, AliasEntry, CallAtom, CapabilityParam, ContainerDef, DirectEffects,
    EffectRowEntry, ExternalFnDef, GlobalVarDef, LineEntry, ListDef, ListItemDef, ScopeLineTable,
    StructShapeDef,
};
use crate::id::DefinitionId;
use crate::line::{LineContent, LinePart, PluralCategory, SelectKey};
use crate::story::StoryData;
use crate::value::{ListValue, MapKey, ProjSegment, Value, ValueType};

use super::{
    CAP_PARAM_ANY, CAT_FEW, CAT_MANY, CAT_ONE, CAT_OTHER, CAT_TWO, CAT_ZERO, HANDLE_PARAM_NONE,
    HEADER_PREAMBLE, KEY_CARDINAL, KEY_EXACT, KEY_KEYWORD, KEY_ORDINAL, LINE_PLAIN, LINE_TEMPLATE,
    MAGIC, PART_LITERAL, PART_SELECT, PART_SLOT, PROJ_SEG_INDEX, PROJ_SEG_KEY, SECTION_COUNT,
    SECTION_ENTRY_SIZE, SectionKind, VAL_ARRAY, VAL_BOOL, VAL_CLOSURE, VAL_DIVERT_TARGET,
    VAL_FLOAT, VAL_FN_REF, VAL_FRAGMENT_REF, VAL_HANDLE, VAL_INT, VAL_LIST, VAL_MAP, VAL_NULL,
    VAL_PROJECTION, VAL_RECORD, VAL_STRING, VAL_VAR_POINTER, VERSION,
};

// ── Tier 1: Full story write ────────────────────────────────────────────────

/// Encode a [`StoryData`] into the `.inkb` binary format with sectioned header.
#[expect(clippy::cast_possible_truncation)]
pub fn write_inkb(story: &StoryData, buf: &mut Vec<u8>) {
    let base = buf.len();

    // The `Visibility` section (M-2b, tag `0x0E`) is **optional**: emitted
    // only when the story has `#@private` definitions. All-public stories —
    // the entire pre-modules world — omit it, so their offset table stays
    // at `SECTION_COUNT` entries (which now includes the mandatory M-3
    // `AliasTable` section, always present — possibly empty — from v5
    // onward; see `SectionKind::AliasTable`).
    let has_visibility = !story.private_defs.is_empty();
    let section_count = SECTION_COUNT as usize + usize::from(has_visibility);
    let header_size = HEADER_PREAMBLE + section_count * SECTION_ENTRY_SIZE;

    // Write placeholder header (zeros) — we'll patch it after writing sections.
    buf.resize(base + header_size, 0);

    // Track (kind, offset) pairs as we write each section, in canonical
    // tag order. The offset table is self-describing (count + per-entry tag),
    // so a conditionally-omitted section is fully readable.
    let mut sections: Vec<(SectionKind, u32)> = Vec::with_capacity(section_count);

    macro_rules! section {
        ($kind:expr, $write:expr) => {{
            let offset = (buf.len() - base) as u32;
            $write;
            sections.push(($kind, offset));
        }};
    }

    section!(
        SectionKind::NameTable,
        write_section_name_table(&story.name_table, buf)
    );
    section!(
        SectionKind::Variables,
        write_section_variables(&story.variables, buf)
    );
    section!(
        SectionKind::ListDefs,
        write_section_list_defs(&story.list_defs, buf)
    );
    section!(
        SectionKind::ListItems,
        write_section_list_items(&story.list_items, buf)
    );
    section!(
        SectionKind::Externals,
        write_section_externals(&story.externals, buf)
    );
    section!(
        SectionKind::Containers,
        write_section_containers(&story.containers, buf)
    );
    section!(
        SectionKind::LineTables,
        write_section_line_tables(&story.line_tables, buf)
    );
    section!(
        SectionKind::Labels,
        write_section_addresses(&story.addresses, buf)
    );
    section!(
        SectionKind::ListLiterals,
        write_section_list_literals(&story.list_literals, buf)
    );
    section!(
        SectionKind::AddressPaths,
        write_section_address_paths(&story.address_paths, buf)
    );
    section!(
        SectionKind::LiteralPool,
        write_section_literal_pool(&story.literal_pool, buf)
    );
    section!(
        SectionKind::StructShapes,
        write_section_struct_shapes(&story.struct_shapes, buf)
    );
    // EffectRows (T2-3, tag 0x0D) is mandatory — always present (possibly
    // empty), section-locally versioned. Emitted between StructShapes and the
    // optional Visibility section so tags stay in canonical ascending order.
    section!(
        SectionKind::EffectRows,
        write_section_effect_rows(&story.effect_rows, buf)
    );
    if has_visibility {
        section!(
            SectionKind::Visibility,
            write_section_visibility(&story.private_defs, buf)
        );
    }
    // AliasTable (M-3) is mandatory — always present (possibly empty) from
    // v5 onward, unlike the optional `Visibility` section above.
    section!(
        SectionKind::AliasTable,
        write_section_alias_table(&story.alias_table, buf)
    );

    let file_size = (buf.len() - base) as u32;
    let checksum = crc32(&buf[base + header_size..]);

    // Patch header in-place.
    let h = &mut buf[base..];
    h[0..4].copy_from_slice(MAGIC);
    h[4..6].copy_from_slice(&VERSION.to_le_bytes());
    h[6] = section_count as u8;
    h[7] = 0; // reserved
    h[8..12].copy_from_slice(&file_size.to_le_bytes());
    h[12..16].copy_from_slice(&checksum.to_le_bytes());

    for (i, (kind, offset)) in sections.iter().enumerate() {
        let entry_base = HEADER_PREAMBLE + i * SECTION_ENTRY_SIZE;
        h[entry_base] = *kind as u8;
        h[entry_base + 1] = 0; // reserved
        h[entry_base + 2] = 0;
        h[entry_base + 3] = 0;
        h[entry_base + 4..entry_base + 8].copy_from_slice(&offset.to_le_bytes());
    }
}

// ── Assembly ────────────────────────────────────────────────────────────────

/// Assemble a complete `.inkb` file from pre-encoded section buffers.
///
/// Sections should be provided in the canonical order matching [`SectionKind`]
/// tags. The header (with offsets and checksum) is computed automatically.
#[expect(clippy::cast_possible_truncation)]
pub fn assemble_inkb(sections: &[(SectionKind, &[u8])], out: &mut Vec<u8>) {
    let base = out.len();
    let section_count = sections.len() as u8;
    let header_size = HEADER_PREAMBLE + sections.len() * SECTION_ENTRY_SIZE;

    // Placeholder header.
    out.resize(base + header_size, 0);

    // Append section data and record offsets.
    let mut entries: Vec<(SectionKind, u32)> = Vec::with_capacity(sections.len());
    for (kind, data) in sections {
        let offset = (out.len() - base) as u32;
        entries.push((*kind, offset));
        out.extend_from_slice(data);
    }

    let file_size = (out.len() - base) as u32;
    let checksum = crc32(&out[base + header_size..]);

    // Patch header.
    let h = &mut out[base..];
    h[0..4].copy_from_slice(MAGIC);
    h[4..6].copy_from_slice(&VERSION.to_le_bytes());
    h[6] = section_count;
    h[7] = 0;
    h[8..12].copy_from_slice(&file_size.to_le_bytes());
    h[12..16].copy_from_slice(&checksum.to_le_bytes());

    for (i, (kind, offset)) in entries.iter().enumerate() {
        let entry_base = HEADER_PREAMBLE + i * SECTION_ENTRY_SIZE;
        h[entry_base] = *kind as u8;
        h[entry_base + 1] = 0;
        h[entry_base + 2] = 0;
        h[entry_base + 3] = 0;
        h[entry_base + 4..entry_base + 8].copy_from_slice(&offset.to_le_bytes());
    }
}

// ── Section writers ─────────────────────────────────────────────────────────

/// Write the name table section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_name_table(names: &[String], buf: &mut Vec<u8>) {
    write_u32(buf, names.len() as u32);
    for name in names {
        write_str(buf, name);
    }
}

/// Write the variables section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_variables(variables: &[GlobalVarDef], buf: &mut Vec<u8>) {
    write_u32(buf, variables.len() as u32);
    for var in variables {
        encode_global_var(var, buf);
    }
}

/// Write the list definitions section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_list_defs(list_defs: &[ListDef], buf: &mut Vec<u8>) {
    write_u32(buf, list_defs.len() as u32);
    for ld in list_defs {
        encode_list_def(ld, buf);
    }
}

/// Write the list items section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_list_items(list_items: &[ListItemDef], buf: &mut Vec<u8>) {
    write_u32(buf, list_items.len() as u32);
    for li in list_items {
        encode_list_item(li, buf);
    }
}

/// Write the externals section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_externals(externals: &[ExternalFnDef], buf: &mut Vec<u8>) {
    write_u32(buf, externals.len() as u32);
    for ext in externals {
        encode_external(ext, buf);
    }
}

/// Write the containers section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_containers(containers: &[ContainerDef], buf: &mut Vec<u8>) {
    write_u32(buf, containers.len() as u32);
    for c in containers {
        encode_container(c, buf);
    }
}

/// Write the addresses section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_addresses(addresses: &[AddressDef], buf: &mut Vec<u8>) {
    write_u32(buf, addresses.len() as u32);
    for addr in addresses {
        write_def_id(buf, addr.id);
        write_def_id(buf, addr.container_id);
        write_u32(buf, addr.byte_offset);
    }
}

/// Write the address-paths section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_address_paths(address_paths: &[AddressPath], buf: &mut Vec<u8>) {
    write_u32(buf, address_paths.len() as u32);
    for ap in address_paths {
        write_u16(buf, ap.path.0);
        write_def_id(buf, ap.target);
    }
}

/// Write the visibility section (no header framing): a count followed by the
/// `DefinitionId` of every `#@private` definition (M-2b). Callers only emit
/// this section when `private_defs` is non-empty.
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_visibility(private_defs: &[DefinitionId], buf: &mut Vec<u8>) {
    write_u32(buf, private_defs.len() as u32);
    for id in private_defs {
        write_def_id(buf, *id);
    }
}

// ── Encode helpers (private) ────────────────────────────────────────────────

fn encode_global_var(v: &GlobalVarDef, buf: &mut Vec<u8>) {
    write_def_id(buf, v.id);
    write_u16(buf, v.name.0);
    encode_value_type(v.value_type, buf);
    encode_value(&v.default_value, buf);
    write_u8(buf, u8::from(v.mutable));
    write_u8(buf, u8::from(v.local));
}

fn encode_value_type(vt: ValueType, buf: &mut Vec<u8>) {
    let tag = match vt {
        ValueType::Int => VAL_INT,
        ValueType::Float => VAL_FLOAT,
        ValueType::Bool => VAL_BOOL,
        ValueType::String => VAL_STRING,
        ValueType::List => VAL_LIST,
        ValueType::DivertTarget => VAL_DIVERT_TARGET,
        ValueType::VariablePointer => VAL_VAR_POINTER,
        // TempPointer is runtime-only and should never appear in .inkb files.
        ValueType::FragmentRef => VAL_FRAGMENT_REF,
        ValueType::TempPointer | ValueType::Null => VAL_NULL,
        // Collection value types (v4, `docs/format-v4-rfc.md` §1).
        ValueType::Array => VAL_ARRAY,
        ValueType::Map => VAL_MAP,
        // TM-4 record value type (v4, reserved tag graduated this PR).
        ValueType::Record => VAL_RECORD,
        // T1c function value types (v4, materialized in #700).
        ValueType::FnRef => VAL_FN_REF,
        ValueType::Closure => VAL_CLOSURE,
        // T1d handle value type (v4, reserved tag graduated this PR).
        ValueType::Handle => VAL_HANDLE,
        // T1e projection value type (v4, reserved tag graduated this PR).
        ValueType::Projection => VAL_PROJECTION,
    };
    write_u8(buf, tag);
}

#[expect(clippy::cast_possible_truncation)]
fn encode_value(v: &Value, buf: &mut Vec<u8>) {
    match v {
        Value::Int(n) => {
            write_u8(buf, VAL_INT);
            write_i32(buf, *n);
        }
        Value::Float(n) => {
            write_u8(buf, VAL_FLOAT);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        Value::Bool(b) => {
            write_u8(buf, VAL_BOOL);
            write_u8(buf, u8::from(*b));
        }
        Value::String(s) => {
            write_u8(buf, VAL_STRING);
            write_str(buf, s);
        }
        Value::List(lv) => {
            write_u8(buf, VAL_LIST);
            write_u32(buf, lv.items.len() as u32);
            for item in &lv.items {
                write_def_id(buf, *item);
            }
            write_u32(buf, lv.origins.len() as u32);
            for origin in &lv.origins {
                write_def_id(buf, *origin);
            }
        }
        Value::DivertTarget(id) => {
            write_u8(buf, VAL_DIVERT_TARGET);
            write_def_id(buf, *id);
        }
        Value::VariablePointer(id) => {
            write_u8(buf, VAL_VAR_POINTER);
            write_def_id(buf, *id);
        }
        Value::FragmentRef(idx) => {
            write_u8(buf, VAL_FRAGMENT_REF);
            write_u32(buf, *idx);
        }
        // TempPointer is runtime-only and should never appear in .inkb files.
        Value::TempPointer { .. } | Value::Null => {
            write_u8(buf, VAL_NULL);
        }
        // Collections encode as trees (v4, `docs/format-v4-rfc.md` §1): a length
        // prefix then the recursively-encoded elements / key-value pairs. Arc
        // sharing is deliberately not preserved on the wire (value-model-spec §5).
        Value::Array(items) => {
            write_u8(buf, VAL_ARRAY);
            write_u32(buf, items.len() as u32);
            for item in items.iter() {
                encode_value(item, buf);
            }
        }
        Value::Map(map) => {
            write_u8(buf, VAL_MAP);
            write_u32(buf, map.len() as u32);
            // Insertion order is semantic (keys restricted to int/string/bool).
            for (key, val) in map.iter() {
                encode_map_key(key, buf);
                encode_value(val, buf);
            }
        }
        // TM-4 (`docs/format-v4-rfc.md` §1): `ShapeId` then field values in
        // shape order — no field names on the wire (they live once, in the
        // `StructShapes` section entry the shape id references).
        Value::Record { shape, fields } => {
            write_u8(buf, VAL_RECORD);
            write_u32(buf, shape.0);
            write_u32(buf, fields.len() as u32);
            for field in fields.iter() {
                encode_value(field, buf);
            }
        }
        // Function values (T1c, `docs/format-v4-rfc.md` §1). `FnRef` is just
        // the fn token; `Closure` adds a u16-counted env of `{NameId, kind u8,
        // value}` entries — the named/moded env is the redundancy rehydration
        // validation reads (spec §6).
        Value::FnRef(target) => {
            write_u8(buf, VAL_FN_REF);
            write_def_id(buf, *target);
        }
        Value::Closure(c) => {
            write_u8(buf, VAL_CLOSURE);
            write_def_id(buf, c.target);
            write_u16(buf, c.env.len() as u16);
            for entry in &c.env {
                write_u16(buf, entry.name.0);
                write_u8(buf, u8::from(entry.is_ref));
                encode_value(&entry.payload, buf);
            }
        }
        // Handle values (T1d, `docs/format-v4-rfc.md` §1: `kind NameId, u64
        // id`). First emission of this reserved tag — the wire form is frozen
        // by the RFC, materialized here. No opcode ever pushes one; a handle
        // reaches this encoder only as a binding-produced global default or a
        // literal-pool entry supplied by a future manifest-aware pipeline.
        Value::Handle { kind, id } => {
            write_u8(buf, VAL_HANDLE);
            write_u16(buf, kind.0);
            write_u64(buf, *id);
        }
        // Projection values (T1e, `docs/format-v4-rfc.md` §1: "cell
        // reference, u8 segment count, then segments"). First emission of
        // this reserved tag. Segment kind `2=range` is RESERVED and never
        // written — `ProjSegment` has no variant to produce it.
        Value::Projection(p) => {
            write_u8(buf, VAL_PROJECTION);
            write_def_id(buf, p.cell);
            write_u8(buf, p.segments.len() as u8);
            for seg in &p.segments {
                encode_proj_segment(seg, buf);
            }
        }
    }
}

/// Encode a single [`ProjSegment`] (`docs/format-v4-rfc.md` §1: `u8 kind (0
/// = index i32, 1 = key value)`).
fn encode_proj_segment(seg: &ProjSegment, buf: &mut Vec<u8>) {
    match seg {
        ProjSegment::Index(n) => {
            write_u8(buf, PROJ_SEG_INDEX);
            write_i32(buf, *n);
        }
        ProjSegment::Key(v) => {
            write_u8(buf, PROJ_SEG_KEY);
            encode_value(v, buf);
        }
    }
}

/// Encode a [`MapKey`] using the scalar `VAL_*` tag surface it maps onto
/// (`int`/`string`/`bool` — the v1 key domain, `docs/value-model-spec.md` §4).
/// Self-describing so the reader can reject a non-scalar key tag.
fn encode_map_key(key: &MapKey, buf: &mut Vec<u8>) {
    match key {
        MapKey::Int(n) => {
            write_u8(buf, VAL_INT);
            write_i32(buf, *n);
        }
        MapKey::Str(s) => {
            write_u8(buf, VAL_STRING);
            write_str(buf, s);
        }
        MapKey::Bool(b) => {
            write_u8(buf, VAL_BOOL);
            write_u8(buf, u8::from(*b));
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
fn encode_list_def(ld: &ListDef, buf: &mut Vec<u8>) {
    write_def_id(buf, ld.id);
    write_u16(buf, ld.name.0);
    write_u32(buf, ld.items.len() as u32);
    for (name_id, ordinal) in &ld.items {
        write_u16(buf, name_id.0);
        write_i32(buf, *ordinal);
    }
}

fn encode_list_item(li: &ListItemDef, buf: &mut Vec<u8>) {
    write_def_id(buf, li.id);
    write_def_id(buf, li.origin);
    write_i32(buf, li.ordinal);
    write_u16(buf, li.name.0);
}

/// Write the list literals section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_list_literals(list_literals: &[ListValue], buf: &mut Vec<u8>) {
    write_u32(buf, list_literals.len() as u32);
    for lv in list_literals {
        write_u32(buf, lv.items.len() as u32);
        for item in &lv.items {
            write_def_id(buf, *item);
        }
        write_u32(buf, lv.origins.len() as u32);
        for origin in &lv.origins {
            write_def_id(buf, *origin);
        }
    }
}

/// Write the T1b literal pool section (no header framing) — a flat list of
/// content-hash-deduplicated constant [`Value`]s referenced by
/// `PushLiteral(idx)` (`docs/format-v4-rfc.md` §2). Each entry uses the
/// existing generic `encode_value` (the same recursive `VAL_ARRAY`/`VAL_MAP`
/// tree encoding as a `GlobalVarDef` default).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_literal_pool(literal_pool: &[Value], buf: &mut Vec<u8>) {
    write_u32(buf, literal_pool.len() as u32);
    for v in literal_pool {
        encode_value(v, buf);
    }
}

/// Write the TM-4 `StructShapes` section (no header framing): one entry per
/// declared `STRUCT` — shape id, name, then its ordered field `NameId`s
/// (`docs/format-v4-rfc.md` §2). Empty (count 0) until a compiler milestone
/// emits struct declarations — see the PR description's scope note.
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_struct_shapes(struct_shapes: &[StructShapeDef], buf: &mut Vec<u8>) {
    write_u32(buf, struct_shapes.len() as u32);
    for shape in struct_shapes {
        write_u32(buf, shape.id.0);
        write_u16(buf, shape.name.0);
        write_u16(buf, shape.fields.len() as u16);
        for field in &shape.fields {
            write_u16(buf, field.0);
        }
    }
}

/// Section-local encoding version for `AliasTable` (`docs/modules-spec.md`
/// §5) — independent of the `.inkb` format `VERSION`, so the row encoding
/// can change without another whole-format bump.
pub(crate) const ALIAS_TABLE_SECTION_VERSION: u8 = 1;

/// Write the M-3 `AliasTable` section (no header framing): a one-byte
/// section-local version, then a flat list of old→new `DefinitionId` pairs
/// (`docs/modules-spec.md` §5). Entries are written in the order given —
/// callers sort by `old` for the runtime's binary-search lookup.
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_alias_table(entries: &[AliasEntry], buf: &mut Vec<u8>) {
    write_u8(buf, ALIAS_TABLE_SECTION_VERSION);
    write_u32(buf, entries.len() as u32);
    for entry in entries {
        write_def_id(buf, entry.old);
        write_def_id(buf, entry.new);
    }
}

/// Section-local encoding version for `EffectRows` (T2-3,
/// `docs/effects-spec.md` §11) — independent of the `.inkb` format `VERSION`,
/// so the factored-row encoding can change without another whole-format bump
/// (the reservation this section graduates was made for exactly this).
///
/// Bumped 1 → 2 for #882: each row gains a leading `is_entry` byte (the
/// freeze bit — see [`EffectRowEntry::is_entry`]).
pub(crate) const EFFECT_ROWS_SECTION_VERSION: u8 = 2;

/// Write the T2-3 `EffectRows` section (no header framing): a one-byte
/// section-local version, then the `DefinitionId → row` table of factored
/// effect rows (`docs/effects-spec.md` §11). One entry per knot/stitch — the
/// host's resume-scheduling estimate (§12.1). Entries are written in the order
/// given; callers sort by `def` for determinism.
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_effect_rows(rows: &[EffectRowEntry], buf: &mut Vec<u8>) {
    write_u8(buf, EFFECT_ROWS_SECTION_VERSION);
    write_u32(buf, rows.len() as u32);
    for row in rows {
        write_def_id(buf, row.def);
        // #882 freeze bit: whether this row is a legitimate host entry point
        // (see `EffectRowEntry::is_entry`'s doc — `false` only for
        // `#@private` defs, and the row still ships either way).
        write_u8(buf, u8::from(row.is_entry));
        encode_direct_effects(&row.direct, buf);
        // Per-dispatch entries (v1 emits none, but the encoding ships the
        // structure — a flat row forecloses §7 narrowing).
        write_u32(buf, row.dispatches.len() as u32);
        for d in &row.dispatches {
            write_def_id(buf, d.cell);
            write_u8(buf, u8::from(d.narrowable));
            encode_direct_effects(&d.fallback, buf);
        }
    }
}

/// Encode a [`DirectEffects`] block: reads, writes, call atoms, opaque flag.
#[expect(clippy::cast_possible_truncation)]
fn encode_direct_effects(direct: &DirectEffects, buf: &mut Vec<u8>) {
    write_u32(buf, direct.reads.len() as u32);
    for id in &direct.reads {
        write_def_id(buf, *id);
    }
    write_u32(buf, direct.writes.len() as u32);
    for id in &direct.writes {
        write_def_id(buf, *id);
    }
    write_u32(buf, direct.calls.len() as u32);
    for atom in &direct.calls {
        encode_call_atom(atom, buf);
    }
    write_u8(buf, u8::from(direct.opaque));
}

/// Encode a single [`CallAtom`]: interned name, the capability-parameter slot
/// (`(any)` in v1), then the reserved handle-parameter slot (`None` in v1 —
/// `docs/t1d-spec.md` §7). A bound handle is never emitted in this section
/// version.
fn encode_call_atom(atom: &CallAtom, buf: &mut Vec<u8>) {
    write_u16(buf, atom.name.0);
    let cap_tag = match atom.capability {
        CapabilityParam::Any => CAP_PARAM_ANY,
    };
    write_u8(buf, cap_tag);
    // Reserved handle-parameter slot: v1 is always `None`. A `Some` is
    // structurally representable but never encoded in this section version.
    write_u8(buf, atom.handle_param.unwrap_or(HANDLE_PARAM_NONE));
}

fn encode_external(ext: &ExternalFnDef, buf: &mut Vec<u8>) {
    write_def_id(buf, ext.id);
    write_u16(buf, ext.name.0);
    write_u8(buf, ext.arg_count);
    match ext.fallback {
        Some(fb) => {
            write_u8(buf, 1);
            write_def_id(buf, fb);
        }
        None => {
            write_u8(buf, 0);
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
fn encode_container(c: &ContainerDef, buf: &mut Vec<u8>) {
    write_def_id(buf, c.id);
    write_def_id(buf, c.scope_id);
    match c.name {
        Some(name_id) => {
            write_u8(buf, 1);
            write_u16(buf, name_id.0);
        }
        None => {
            write_u8(buf, 0);
        }
    }
    write_u8(buf, c.counting_flags.bits());
    write_i32(buf, c.path_hash);
    write_u8(buf, c.param_count);
    write_u8(buf, u8::from(c.local));
    // Per-param name/mode metadata (T1c, `docs/t1c-spec.md` §6). Additive
    // trailing field: a `0` count for the common no-param container.
    write_u16(buf, c.params.len() as u16);
    for p in &c.params {
        write_u16(buf, p.name.0);
        write_u8(buf, u8::from(p.is_ref));
    }
    write_u32(buf, c.bytecode.len() as u32);
    buf.extend_from_slice(&c.bytecode);
}

/// Write the line tables section (no header framing).
#[expect(clippy::cast_possible_truncation)]
pub fn write_section_line_tables(line_tables: &[ScopeLineTable], buf: &mut Vec<u8>) {
    write_u32(buf, line_tables.len() as u32);
    for lt in line_tables {
        encode_scope_line_table(lt, buf);
    }
}

#[expect(clippy::cast_possible_truncation)]
fn encode_scope_line_table(lt: &ScopeLineTable, buf: &mut Vec<u8>) {
    write_def_id(buf, lt.scope_id);
    write_u32(buf, lt.lines.len() as u32);
    for entry in &lt.lines {
        encode_line_entry(entry, buf);
    }
}

fn encode_line_entry(entry: &LineEntry, buf: &mut Vec<u8>) {
    encode_line_content(&entry.content, buf);
    write_u64(buf, entry.source_hash);
    match &entry.audio_ref {
        Some(audio) => {
            write_u8(buf, 1);
            write_str(buf, audio);
        }
        None => {
            write_u8(buf, 0);
        }
    }

    // Slot info
    #[expect(clippy::cast_possible_truncation)]
    write_u8(buf, entry.slot_info.len() as u8);
    for slot in &entry.slot_info {
        write_u8(buf, slot.index);
        write_str(buf, &slot.name);
    }

    // Source location
    match &entry.source_location {
        Some(loc) => {
            write_u8(buf, 1);
            write_str(buf, &loc.file);
            write_u32(buf, loc.range_start);
            write_u32(buf, loc.range_end);
        }
        None => {
            write_u8(buf, 0);
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
pub(crate) fn encode_line_content(content: &LineContent, buf: &mut Vec<u8>) {
    match content {
        LineContent::Plain(s) => {
            write_u8(buf, LINE_PLAIN);
            write_str(buf, s);
        }
        LineContent::Template(parts) => {
            write_u8(buf, LINE_TEMPLATE);
            write_u32(buf, parts.len() as u32);
            for part in parts {
                encode_line_part(part, buf);
            }
        }
    }
}

#[expect(clippy::cast_possible_truncation)]
fn encode_line_part(part: &LinePart, buf: &mut Vec<u8>) {
    match part {
        LinePart::Literal(s) => {
            write_u8(buf, PART_LITERAL);
            write_str(buf, s);
        }
        LinePart::Slot(idx) => {
            write_u8(buf, PART_SLOT);
            write_u8(buf, *idx);
        }
        LinePart::Select {
            slot,
            variants,
            default,
        } => {
            write_u8(buf, PART_SELECT);
            write_u8(buf, *slot);
            write_u32(buf, variants.len() as u32);
            for (key, text) in variants {
                encode_select_key(key, buf);
                write_str(buf, text);
            }
            write_str(buf, default);
        }
    }
}

fn encode_select_key(key: &SelectKey, buf: &mut Vec<u8>) {
    match key {
        SelectKey::Cardinal(cat) => {
            write_u8(buf, KEY_CARDINAL);
            encode_plural_category(*cat, buf);
        }
        SelectKey::Ordinal(cat) => {
            write_u8(buf, KEY_ORDINAL);
            encode_plural_category(*cat, buf);
        }
        SelectKey::Exact(n) => {
            write_u8(buf, KEY_EXACT);
            write_i32(buf, *n);
        }
        SelectKey::Keyword(k) => {
            write_u8(buf, KEY_KEYWORD);
            write_str(buf, k);
        }
    }
}

fn encode_plural_category(cat: PluralCategory, buf: &mut Vec<u8>) {
    let tag = match cat {
        PluralCategory::Zero => CAT_ZERO,
        PluralCategory::One => CAT_ONE,
        PluralCategory::Two => CAT_TWO,
        PluralCategory::Few => CAT_FEW,
        PluralCategory::Many => CAT_MANY,
        PluralCategory::Other => CAT_OTHER,
    };
    write_u8(buf, tag);
}
