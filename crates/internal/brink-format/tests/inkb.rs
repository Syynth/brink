#![allow(clippy::unwrap_used)]

use std::path::Path;

use brink_format::{
    DecodeError, MAX_DECODE_DEPTH, SectionKind, assemble_inkb, read_inkb, read_inkb_index,
    read_section_addresses, read_section_containers, read_section_externals,
    read_section_line_tables, read_section_list_defs, read_section_list_items,
    read_section_list_literals, read_section_literal_pool, read_section_name_table,
    read_section_variables, write_inkb, write_section_address_paths, write_section_addresses,
    write_section_alias_table, write_section_containers, write_section_effect_rows,
    write_section_externals, write_section_line_tables, write_section_list_defs,
    write_section_list_items, write_section_list_literals, write_section_literal_pool,
    write_section_name_table, write_section_struct_shapes, write_section_variables,
};

fn i001_data() -> brink_format::StoryData {
    // Compile from an in-memory string with a fixed entry name so the
    // embedded source path (and thus snapshots) stay machine-independent.
    let src = include_str!("../../../../tests/tier1/basics/I001-minimal-story/story.ink");
    brink_compiler::compile("story.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data
}

#[test]
fn roundtrip_i001_minimal_story() {
    let data = i001_data();

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);
}

// ── M-2b visibility section (tag 0x0E) ──────────────────────────────────────

#[test]
fn roundtrip_visibility_section() {
    use brink_format::{DefinitionId, DefinitionTag};

    let mut data = i001_data();
    // Sorted ascending, as the compiler emits.
    let mut private_defs = vec![
        DefinitionId::new(DefinitionTag::GlobalVar, 3),
        DefinitionId::new(DefinitionTag::Address, 7),
    ];
    private_defs.sort_by_key(|id| id.to_raw());
    data.private_defs = private_defs;

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    // The optional section shows up in the offset table when non-empty.
    let index = read_inkb_index(&buf).unwrap();
    assert!(
        index
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::Visibility)
    );

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data.private_defs, recovered.private_defs);
    assert_eq!(data, recovered);
}

#[test]
fn visibility_section_omitted_when_empty() {
    // The all-public common case: no Visibility section, so an all-public
    // story's offset table stays at the mandatory `SECTION_COUNT` entries —
    // Visibility itself needs no version bump. (The whole-file `VERSION` is
    // 6 regardless, because of the unrelated *mandatory* M-3 `AliasTable`
    // section and the v6 `PART_SPAN` bump — see
    // `roundtrip_alias_table`/`missing_alias_table_section_decodes_empty`
    // and `roundtrip_line_part_span`.)
    let data = i001_data();
    assert!(data.private_defs.is_empty());

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    assert!(
        !index
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::Visibility)
    );
    assert_eq!(index.version, 6);
}

// ── v4 collection value encoding (#526) ─────────────────────────────────────
//
// The v4 format serializes `Value::Array`/`Value::Map` as trees in the
// Variables section (and everywhere else `encode_value` is reached). No opcode
// emits a collection literal yet — that is T1b — so this test hand-injects
// collection-valued globals and round-trips the whole `.inkb`, proving the new
// tag arms encode and decode structurally (insertion order + scalar key types
// preserved) instead of the pre-v4 fold-to-`VAL_NULL` placeholder.
#[test]
fn roundtrip_collection_valued_globals() {
    use brink_format::{
        DefinitionId, DefinitionTag, GlobalVarDef, MapKey, NameId, OrderedMap, Value, ValueType,
    };

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;

    // A nested array: [1, "two", [true, null]].
    let arr = Value::array(vec![
        Value::Int(1),
        Value::String("two".into()),
        Value::array(vec![Value::Bool(true), Value::Null]),
    ]);
    // A map with all three scalar key kinds in a deliberately non-sorted order,
    // nesting an array value.
    let map: OrderedMap = [
        (MapKey::from("hp"), Value::Int(30)),
        (
            MapKey::from(7),
            Value::array(vec![Value::Int(1), Value::Int(2)]),
        ),
        (MapKey::from(true), Value::String("flag".into())),
    ]
    .into_iter()
    .collect();

    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Array,
        default_value: arr,
        mutable: true,
        local: false,
    });
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id + 1),
        name: NameId(0),
        value_type: ValueType::Map,
        default_value: Value::map(map),
        mutable: true,
        local: false,
    });

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);
}

/// T1d (`docs/t1d-spec.md` §2, `docs/format-v4-rfc.md` §1): the first
/// emission of the reserved `VAL_HANDLE` wire tag — write→read identity for
/// a `Handle`-valued global, both bare and nested inside a collection (the
/// wire form is `kind NameId, u64 id`; a max-magnitude id must not lose a
/// single bit crossing the wire).
#[test]
fn roundtrip_handle_valued_globals() {
    use brink_format::{DefinitionId, DefinitionTag, GlobalVarDef, NameId, Value, ValueType};

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;

    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Handle,
        default_value: Value::handle(NameId(3), u64::MAX),
        mutable: true,
        local: false,
    });
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id + 1),
        name: NameId(0),
        value_type: ValueType::Array,
        default_value: Value::array(vec![
            Value::handle(NameId(1), 0),
            Value::handle(NameId(2), 42),
        ]),
        mutable: true,
        local: false,
    });

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);
}

/// T1e (`docs/t1e-spec.md` §3, `docs/format-v4-rfc.md` §1): the first
/// emission of the reserved `VAL_PROJECTION` wire tag — write→read identity
/// for a `Projection`-valued global, both a mixed index+field-name segment
/// chain and one nested inside a collection. Segment kind `2=range` stays
/// RESERVED (never constructed — `ProjSegment` has no such variant) — this
/// test only proves the two kinds that ARE emitted (`0=index`, `1=key`)
/// round-trip byte-for-byte.
#[test]
fn roundtrip_projection_valued_globals() {
    use brink_format::{
        DefinitionId, DefinitionTag, GlobalVarDef, NameId, ProjSegment, Value, ValueType,
    };

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;
    let cell = DefinitionId::new(DefinitionTag::GlobalVar, 999);

    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Projection,
        default_value: Value::projection(
            cell,
            vec![
                ProjSegment::Key(Value::String("hp".into())),
                ProjSegment::Index(3),
                ProjSegment::Key(Value::Bool(true)),
            ],
        ),
        mutable: true,
        local: false,
    });
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id + 1),
        name: NameId(0),
        value_type: ValueType::Array,
        default_value: Value::array(vec![
            Value::projection(cell, vec![]),
            Value::projection(cell, vec![ProjSegment::Index(i32::MIN)]),
        ]),
        mutable: true,
        local: false,
    });

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);
}

/// M-3 `AliasTable` section (`docs/modules-spec.md` §5, format section tag
/// `0x0F`): write→read identity for the compiled `#@was` alias table —
/// several entries of mixed `DefinitionTag`s (a knot rename and a global
/// var rename both produce entries; their tags differ), proving the
/// section round-trips through the binary format byte-for-byte.
#[test]
fn roundtrip_alias_table() {
    use brink_format::{AliasEntry, DefinitionId, DefinitionTag};

    let mut data = i001_data();
    data.alias_table = vec![
        AliasEntry {
            old: DefinitionId::new(DefinitionTag::Address, 1),
            new: DefinitionId::new(DefinitionTag::Address, 2),
        },
        AliasEntry {
            old: DefinitionId::new(DefinitionTag::GlobalVar, 3),
            new: DefinitionId::new(DefinitionTag::GlobalVar, 4),
        },
        AliasEntry {
            old: DefinitionId::new(DefinitionTag::Address, u64::MAX),
            new: DefinitionId::new(DefinitionTag::Address, 0),
        },
    ];

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);
}

// ── T2-3 EffectRows section (tag 0x0D) ──────────────────────────────────────

/// A factored `EffectRows` table — direct part (reads/writes/call atoms/opaque)
/// plus a per-dispatch entry with a narrowable bit and a static-fallback row —
/// round-trips exactly through `.inkb`. Exercises the reader's dispatch path
/// even though v1 emission produces none (writer and reader land together, the
/// #742 lesson).
#[test]
fn roundtrip_effect_rows() {
    use brink_format::{
        CallAtom, CapabilityParam, DefinitionId, DefinitionTag, DirectEffects, DispatchEntry,
        EffectRowEntry, NameId,
    };

    let cell = |n| DefinitionId::new(DefinitionTag::GlobalVar, n);
    let mut data = i001_data();
    data.effect_rows = vec![
        EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 1),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![cell(1), cell(2)],
                writes: vec![cell(3)],
                calls: vec![
                    CallAtom {
                        name: NameId(5),
                        capability: CapabilityParam::Any,
                        handle_param: None,
                    },
                    CallAtom {
                        name: NameId(6),
                        capability: CapabilityParam::Any,
                        handle_param: None,
                    },
                ],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![DispatchEntry {
                cell: cell(9),
                narrowable: true,
                fallback: DirectEffects {
                    reads: vec![cell(4)],
                    writes: vec![],
                    calls: vec![CallAtom {
                        name: NameId(7),
                        capability: CapabilityParam::Any,
                        handle_param: None,
                    }],
                    opaque: true,
                    emits: false,
                    tags: false,
                    faults: false,
                },
            }],
        },
        EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 2),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![],
                opaque: true,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        },
    ];

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);
}

/// #882: the freeze bit (`EffectRowEntry::is_entry`) round-trips through
/// `.inkb`. A `#@private` def's row (`is_entry: false`) is not dropped from
/// the table — it stays resolvable by `def` after the round trip, alongside
/// an unaffected public row (`docs/effects-spec.md` §10; `docs/modules-spec.md`
/// §4 rule 1: private hides the name, not the cell).
#[test]
fn roundtrip_effect_rows_freeze_bit() {
    use brink_format::{
        CallAtom, CapabilityParam, DefinitionId, DefinitionTag, DirectEffects, EffectRowEntry,
        NameId,
    };

    let private_def = DefinitionId::new(DefinitionTag::Address, 1);
    let public_def = DefinitionId::new(DefinitionTag::Address, 2);
    let mut data = i001_data();
    data.effect_rows = vec![
        EffectRowEntry {
            def: private_def,
            is_entry: false,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![CallAtom {
                    name: NameId(5),
                    capability: CapabilityParam::Any,
                    handle_param: None,
                }],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        },
        EffectRowEntry {
            def: public_def,
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![],
                opaque: false,
                emits: false,
                tags: false,
                faults: false,
            },
            dispatches: vec![],
        },
    ];

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);

    let private_row = recovered
        .effect_rows
        .iter()
        .find(|r| r.def == private_def)
        .expect("private def's row still resolvable via the table");
    assert!(!private_row.is_entry);
    assert_eq!(private_row.direct.calls.len(), 1);

    let public_row = recovered
        .effect_rows
        .iter()
        .find(|r| r.def == public_def)
        .expect("public def's row unaffected");
    assert!(public_row.is_entry);
}

/// A `.inkb` with no `EffectRows` section (converter output, or a pre-T2-3
/// file) decodes the table as empty rather than erroring —
/// `read_section_effect_rows`'s absent-section path.
#[test]
fn missing_effect_rows_section_decodes_empty() {
    use brink_format::{SectionKind, read_inkb_index};

    let mut data = i001_data();
    data.effect_rows = vec![];

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    // Sanity: the section is present (write_inkb always emits it, possibly
    // empty), and an empty table round-trips as empty.
    let index = read_inkb_index(&buf).unwrap();
    assert!(index.section_range(SectionKind::EffectRows).is_some());
    let recovered = read_inkb(&buf).unwrap();
    assert!(recovered.effect_rows.is_empty());
}

/// Build a minimal one-entry `EffectRows` section body with one call atom
/// carrying the given capability and handle-parameter tag bytes, wrapped in a
/// hand-rolled [`brink_format::InkbIndex`] pointing at it. Lets the reserved-slot
/// rejection tests exercise `read_section_effect_rows` directly.
fn effect_rows_index_with_call_tags(
    cap_tag: u8,
    handle_tag: u8,
) -> (Vec<u8>, brink_format::InkbIndex) {
    use brink_format::{DefinitionId, DefinitionTag, InkbIndex, SectionEntry, SectionKind};

    let mut buf = Vec::new();
    buf.push(3u8); // section-local version (NS-A2: bumped 2 -> 3 for dims)
    buf.extend_from_slice(&1u32.to_le_bytes()); // entry count
    buf.extend_from_slice(
        &DefinitionId::new(DefinitionTag::Address, 1)
            .to_raw()
            .to_le_bytes(),
    );
    buf.push(1u8); // is_entry
    buf.extend_from_slice(&0u32.to_le_bytes()); // reads
    buf.extend_from_slice(&0u32.to_le_bytes()); // writes
    buf.extend_from_slice(&1u32.to_le_bytes()); // calls
    buf.extend_from_slice(&5u16.to_le_bytes()); // name
    buf.push(cap_tag);
    buf.push(handle_tag);
    buf.push(0u8); // opaque
    buf.push(0u8); // NS-A2 dims flags (none set)
    buf.extend_from_slice(&0u32.to_le_bytes()); // dispatch count

    let file_size = u32::try_from(buf.len()).unwrap();
    let index = InkbIndex {
        version: 5,
        file_size,
        checksum: 0,
        sections: vec![SectionEntry {
            kind: SectionKind::EffectRows,
            offset: 0,
        }],
    };
    (buf, index)
}

/// The reserved handle-parameter slot (`docs/t1d-spec.md` §7) is rejected when
/// non-zero — v1 emits only `None`, and the strict reader refuses a bound
/// handle, the same reservation discipline the projection range segment follows.
#[test]
fn effect_rows_reader_rejects_reserved_handle_param() {
    let (buf, index) = effect_rows_index_with_call_tags(0x00, 0x01);
    let err = brink_format::read_section_effect_rows(&buf, &index).unwrap_err();
    assert_eq!(err, DecodeError::InvalidEffectHandleParam(0x01));
}

/// A non-`Any` capability-parameter tag (path-granular is reserved, #826) is
/// rejected by the strict reader.
#[test]
fn effect_rows_reader_rejects_reserved_cap_param() {
    let (buf, index) = effect_rows_index_with_call_tags(0x01, 0x00);
    let err = brink_format::read_section_effect_rows(&buf, &index).unwrap_err();
    assert_eq!(err, DecodeError::InvalidEffectCapParam(0x01));
}

/// NS-A2 (issue #1108): the emits/tags/faults dimension flags round-trip
/// through the section-version-3 extension byte.
#[test]
fn effect_rows_dimension_flags_roundtrip() {
    use brink_format::{DefinitionId, DefinitionTag, DirectEffects, EffectRowEntry};

    let mut data = i001_data();
    data.effect_rows = vec![
        EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 1),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![],
                opaque: false,
                emits: true,
                tags: false,
                faults: true,
            },
            dispatches: vec![],
        },
        EffectRowEntry {
            def: DefinitionId::new(DefinitionTag::Address, 2),
            is_entry: true,
            direct: DirectEffects {
                reads: vec![],
                writes: vec![],
                calls: vec![],
                opaque: false,
                emits: false,
                tags: true,
                faults: false,
            },
            dispatches: vec![],
        },
    ];

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let decoded = read_inkb(&buf).expect("decode");
    assert_eq!(decoded.effect_rows, data.effect_rows);
}

/// NS-A2: reserved bits (3-7) in the dimension-flags byte are rejected by
/// the strict reader until a section version graduates them — the same
/// discipline as the capability/handle slots.
#[test]
fn effect_rows_reader_rejects_reserved_dimension_bits() {
    use brink_format::{DefinitionId, DefinitionTag, InkbIndex, SectionEntry, SectionKind};

    let mut buf = Vec::new();
    buf.push(3u8); // section-local version
    buf.extend_from_slice(&1u32.to_le_bytes()); // entry count
    buf.extend_from_slice(
        &DefinitionId::new(DefinitionTag::Address, 1)
            .to_raw()
            .to_le_bytes(),
    );
    buf.push(1u8); // is_entry
    buf.extend_from_slice(&0u32.to_le_bytes()); // reads
    buf.extend_from_slice(&0u32.to_le_bytes()); // writes
    buf.extend_from_slice(&0u32.to_le_bytes()); // calls
    buf.push(0u8); // opaque
    buf.push(0b0000_1000u8); // reserved bit 3 set
    buf.extend_from_slice(&0u32.to_le_bytes()); // dispatch count
    let index = InkbIndex {
        version: 5,
        file_size: u32::try_from(buf.len()).unwrap(),
        checksum: 0,
        sections: vec![SectionEntry {
            kind: SectionKind::EffectRows,
            offset: 0,
        }],
    };
    let err = brink_format::read_section_effect_rows(&buf, &index).unwrap_err();
    assert_eq!(err, DecodeError::InvalidEffectDimensions(0b0000_1000));
}

/// An `EffectRows` section carrying an unknown section-local version byte is
/// rejected (the version prefix is the forward-compat mechanism, independent
/// of the whole-file `VERSION`).
#[test]
fn effect_rows_reader_rejects_unknown_section_version() {
    use brink_format::{InkbIndex, SectionEntry, SectionKind};

    let mut buf = Vec::new();
    buf.push(99u8); // unknown section-local version
    buf.extend_from_slice(&0u32.to_le_bytes());
    let index = InkbIndex {
        version: 5,
        file_size: u32::try_from(buf.len()).unwrap(),
        checksum: 0,
        sections: vec![SectionEntry {
            kind: SectionKind::EffectRows,
            offset: 0,
        }],
    };
    let err = brink_format::read_section_effect_rows(&buf, &index).unwrap_err();
    assert!(
        matches!(
            err,
            DecodeError::UnsupportedSectionVersion { version: 99, .. }
        ),
        "expected UnsupportedSectionVersion, got {err:?}"
    );
}

/// A `.inkb` with no `AliasTable` section at all (a pre-M-3 file, or a
/// story with no `#@was` directives) decodes the alias table as empty
/// rather than erroring — `read_section_alias_table`'s absent-section path.
#[test]
fn missing_alias_table_section_decodes_empty() {
    let data = i001_data();
    assert!(
        data.alias_table.is_empty(),
        "sanity: I001 has no #@was directives"
    );

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let recovered = read_inkb(&buf).unwrap();
    assert!(recovered.alias_table.is_empty());
}

// ── Recursion-depth cap on VAL_ARRAY/VAL_MAP decode (#553, #561, #562) ──────
//
// `decode_value` recurses into itself for VAL_ARRAY/VAL_MAP children with no
// depth limit. A crafted `.inkb` of nested single-element arrays (~5
// bytes/level) can stack-overflow the reader. These tests hand-build a
// `Value` nested exactly at, and one past, the reader's cap
// (`brink_format::MAX_DECODE_DEPTH` — the single canonical definition shared
// by every `decode_value` implementation, #561) and prove the reader accepts
// the former unchanged and rejects the latter with a proper `DecodeError`
// instead of overflowing the stack. Both the `VAL_ARRAY` recursion branch and
// the parallel `VAL_MAP` branch are exercised at the boundary (#562).

/// A `Value` wrapped in `depth` single-element arrays around a scalar leaf,
/// matching the issue's "nested single-element arrays" shape.
fn nested_array(depth: usize) -> brink_format::Value {
    use brink_format::Value;
    let mut v = Value::Int(42);
    for _ in 0..depth {
        v = Value::array(vec![v]);
    }
    v
}

/// A `Value` wrapped in `depth` single-entry maps around a scalar leaf —
/// the `VAL_MAP` analogue of [`nested_array`], exercising the parallel map
/// recursion branch in `decode_value` (#562).
fn nested_map(depth: usize) -> brink_format::Value {
    use brink_format::{MapKey, OrderedMap, Value};
    let mut v = Value::Int(42);
    for _ in 0..depth {
        let mut map = OrderedMap::with_capacity(1);
        map.insert(MapKey::Int(0), v);
        v = Value::map(map);
    }
    v
}

fn story_with_default_value(value: brink_format::Value) -> brink_format::StoryData {
    use brink_format::{DefinitionId, DefinitionTag, GlobalVarDef, NameId, ValueType};

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Array,
        default_value: value,
        mutable: true,
        local: false,
    });
    data
}

#[test]
fn decode_value_accepts_max_depth_nesting() {
    let value = nested_array(MAX_DECODE_DEPTH);
    let data = story_with_default_value(value.clone());

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let recovered = read_inkb(&buf).expect("depth exactly at cap must decode");
    assert_eq!(recovered.variables.last().unwrap().default_value, value);
}

#[test]
fn decode_value_rejects_beyond_max_depth() {
    let value = nested_array(MAX_DECODE_DEPTH + 1);
    let data = story_with_default_value(value);

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    assert!(matches!(
        read_inkb(&buf),
        Err(DecodeError::MaxDepthExceeded(MAX_DECODE_DEPTH))
    ));
}

#[test]
fn decode_value_rejects_deeply_crafted_nesting() {
    // The actual attack scenario the issue describes: a much deeper chain
    // than any legitimate story would produce (well beyond the cap, but
    // shallow enough that constructing/encoding the fixture itself — which
    // has no depth cap, by design; only the untrusted-input decode path is
    // guarded — doesn't hit unrelated recursion limits). The reader must
    // reject it promptly rather than recursing hundreds of frames deep.
    let value = nested_array(8 * MAX_DECODE_DEPTH);
    let data = story_with_default_value(value);

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    assert!(matches!(
        read_inkb(&buf),
        Err(DecodeError::MaxDepthExceeded(MAX_DECODE_DEPTH))
    ));
}

// ── #562: parallel VAL_MAP recursion branch at the boundary ────────────────

#[test]
fn decode_value_accepts_max_depth_map_nesting() {
    let value = nested_map(MAX_DECODE_DEPTH);
    let data = story_with_default_value(value.clone());

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let recovered = read_inkb(&buf).expect("map depth exactly at cap must decode");
    assert_eq!(recovered.variables.last().unwrap().default_value, value);
}

#[test]
fn decode_value_rejects_beyond_max_depth_map_nesting() {
    let value = nested_map(MAX_DECODE_DEPTH + 1);
    let data = story_with_default_value(value);

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    assert!(matches!(
        read_inkb(&buf),
        Err(DecodeError::MaxDepthExceeded(MAX_DECODE_DEPTH))
    ));
}

// The strict reader rejects any version but 6 — a future v7 artifact is not
// silently accepted (the version check runs ahead of the content checksum).
#[test]
fn strict_reader_rejects_non_v6_version() {
    let data = i001_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    assert!(read_inkb(&buf).is_ok(), "v6 buffer reads cleanly");

    // Bump the on-wire version field (bytes 4..6, LE) to 7.
    buf[4] = 7;
    buf[5] = 0;
    assert!(
        matches!(read_inkb(&buf), Err(DecodeError::UnsupportedVersion(7))),
        "a v7 artifact must be rejected as UnsupportedVersion(7)"
    );
}

#[test]
fn snapshot_i001_inkb_bytes() {
    let data = i001_data();

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    insta::assert_snapshot!(format_hex(&buf));
}

fn format_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        write!(out, "{:08x}  ", i * 16).unwrap();
        for (j, byte) in chunk.iter().enumerate() {
            if j == 8 {
                out.push(' ');
            }
            write!(out, "{byte:02x} ").unwrap();
        }
        // Pad to fixed width
        let padding = 16 - chunk.len();
        for j in 0..padding {
            if chunk.len() + j == 8 {
                out.push(' ');
            }
            out.push_str("   ");
        }
        out.push(' ');
        out.push('|');
        for byte in chunk {
            if byte.is_ascii_graphic() || *byte == b' ' {
                out.push(*byte as char);
            } else {
                out.push('.');
            }
        }
        out.push('|');
        out.push('\n');
    }
    out
}

fn collect_story_ink_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_story_ink_files(&path));
            } else if path.file_name().is_some_and(|n| n == "story.ink") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn inkb_roundtrip_corpus_smoke() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests_dir = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests");

    let files = collect_story_ink_files(&tests_dir);
    assert!(
        !files.is_empty(),
        "no story.ink files found in {tests_dir:?}"
    );

    let mut failures = Vec::new();

    for path in &files {
        // Some corpus stories intentionally do not compile — skip those.
        let Ok(output) = brink_compiler::compile_path(path) else {
            continue;
        };
        let data = output.data;

        let mut buf = Vec::new();
        write_inkb(&data, &mut buf);

        match read_inkb(&buf) {
            Ok(mut recovered) => {
                // source_checksum is set from the binary header, not semantic data.
                recovered.source_checksum = data.source_checksum;
                if data != recovered {
                    failures.push(format!("MISMATCH {}", path.display()));
                }
            }
            Err(e) => {
                failures.push(format!("DECODE {}: {e}", path.display()));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{}/{} files failed inkb roundtrip:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

// ── New tests for sectioned header ──────────────────────────────────────────

fn make_test_data() -> brink_format::StoryData {
    i001_data()
}

#[test]
fn index_parsing() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    assert_eq!(index.version, 6);
    assert_eq!(index.file_size as usize, buf.len());
    assert_eq!(index.sections.len(), 14);

    // Sections are in canonical order.
    assert_eq!(index.sections[0].kind, SectionKind::NameTable);
    assert_eq!(index.sections[1].kind, SectionKind::Variables);
    assert_eq!(index.sections[2].kind, SectionKind::ListDefs);
    assert_eq!(index.sections[3].kind, SectionKind::ListItems);
    assert_eq!(index.sections[4].kind, SectionKind::Externals);
    assert_eq!(index.sections[5].kind, SectionKind::Containers);
    assert_eq!(index.sections[6].kind, SectionKind::LineTables);
    assert_eq!(index.sections[7].kind, SectionKind::Labels);
    assert_eq!(index.sections[8].kind, SectionKind::ListLiterals);
    assert_eq!(index.sections[9].kind, SectionKind::AddressPaths);
    assert_eq!(index.sections[10].kind, SectionKind::LiteralPool);
    assert_eq!(index.sections[11].kind, SectionKind::StructShapes);
    assert_eq!(index.sections[12].kind, SectionKind::EffectRows);
    assert_eq!(index.sections[13].kind, SectionKind::AliasTable);

    // Header size is 16 + 8*14 = 128.
    assert_eq!(index.header_size(), 128);

    // First section starts right after header.
    assert_eq!(index.sections[0].offset as usize, index.header_size());

    // Offsets are monotonically increasing.
    for w in index.sections.windows(2) {
        assert!(
            w[0].offset < w[1].offset,
            "section {:?} offset {} >= {:?} offset {}",
            w[0].kind,
            w[0].offset,
            w[1].kind,
            w[1].offset
        );
    }
}

#[test]
fn section_ranges() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();

    // All section ranges should cover the entire post-header region with no gaps.
    let mut covered = index.header_size();
    for entry in &index.sections {
        let range = index.section_range(entry.kind).unwrap();
        assert_eq!(range.start, covered, "gap before section {:?}", entry.kind);
        covered = range.end;
    }
    assert_eq!(covered, index.file_size as usize);
}

#[test]
fn section_level_roundtrip() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();

    let names = read_section_name_table(&buf, &index).unwrap();
    assert_eq!(names, data.name_table);

    let vars = read_section_variables(&buf, &index).unwrap();
    assert_eq!(vars, data.variables);

    let list_defs = read_section_list_defs(&buf, &index).unwrap();
    assert_eq!(list_defs, data.list_defs);

    let list_items = read_section_list_items(&buf, &index).unwrap();
    assert_eq!(list_items, data.list_items);

    let exts = read_section_externals(&buf, &index).unwrap();
    assert_eq!(exts, data.externals);

    let containers = read_section_containers(&buf, &index).unwrap();
    assert_eq!(containers, data.containers);

    let line_tables = read_section_line_tables(&buf, &index).unwrap();
    assert_eq!(line_tables, data.line_tables);

    let addresses = read_section_addresses(&buf, &index).unwrap();
    assert_eq!(addresses, data.addresses);

    let list_literals = read_section_list_literals(&buf, &index).unwrap();
    assert_eq!(list_literals, data.list_literals);

    let literal_pool = read_section_literal_pool(&buf, &index).unwrap();
    assert_eq!(literal_pool, data.literal_pool);
}

/// The T1b literal pool round-trips through `.inkb`, including nested
/// array/map entries (the whole point of the section — `docs/format-v4-rfc.md`
/// §2).
#[test]
fn literal_pool_roundtrip_with_collections() {
    use brink_format::{MapKey, OrderedMap, Value};

    let mut data = i001_data();
    let mut inner_map = OrderedMap::new();
    inner_map.insert(MapKey::from("hp"), Value::Int(10));
    inner_map.insert(MapKey::from(true), Value::String("flag".into()));
    data.literal_pool = vec![
        Value::array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
        Value::map(inner_map),
        Value::array(vec![Value::array(vec![]), Value::Null]),
    ];

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let recovered = read_inkb(&buf).unwrap();
    assert_eq!(recovered.literal_pool, data.literal_pool);
}

#[test]
fn checksum_validation() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    // Corrupt a byte in the section data region.
    let last = buf.len() - 1;
    buf[last] ^= 0xFF;

    let err = read_inkb(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::ChecksumMismatch { .. }),
        "expected ChecksumMismatch, got {err:?}"
    );
}

#[test]
fn assemble_inkb_equivalence() {
    let data = make_test_data();

    // Write via write_inkb.
    let mut direct = Vec::new();
    write_inkb(&data, &mut direct);

    // Write via individual section writers + assemble_inkb.
    let mut name_buf = Vec::new();
    write_section_name_table(&data.name_table, &mut name_buf);

    let mut var_buf = Vec::new();
    write_section_variables(&data.variables, &mut var_buf);

    let mut ld_buf = Vec::new();
    write_section_list_defs(&data.list_defs, &mut ld_buf);

    let mut list_item_buf = Vec::new();
    write_section_list_items(&data.list_items, &mut list_item_buf);

    let mut ext_buf = Vec::new();
    write_section_externals(&data.externals, &mut ext_buf);

    let mut cont_buf = Vec::new();
    write_section_containers(&data.containers, &mut cont_buf);

    let mut line_table_buf = Vec::new();
    write_section_line_tables(&data.line_tables, &mut line_table_buf);

    let mut label_buf = Vec::new();
    write_section_addresses(&data.addresses, &mut label_buf);

    let mut list_lit_buf = Vec::new();
    write_section_list_literals(&data.list_literals, &mut list_lit_buf);

    let mut ap_buf = Vec::new();
    write_section_address_paths(&data.address_paths, &mut ap_buf);

    let mut literal_pool_buf = Vec::new();
    write_section_literal_pool(&data.literal_pool, &mut literal_pool_buf);

    let mut struct_shapes_buf = Vec::new();
    write_section_struct_shapes(&data.struct_shapes, &mut struct_shapes_buf);

    let mut alias_table_buf = Vec::new();
    write_section_alias_table(&data.alias_table, &mut alias_table_buf);

    let mut effect_rows_buf = Vec::new();
    write_section_effect_rows(&data.effect_rows, &mut effect_rows_buf);

    let mut assembled = Vec::new();
    assemble_inkb(
        &[
            (SectionKind::NameTable, &name_buf),
            (SectionKind::Variables, &var_buf),
            (SectionKind::ListDefs, &ld_buf),
            (SectionKind::ListItems, &list_item_buf),
            (SectionKind::Externals, &ext_buf),
            (SectionKind::Containers, &cont_buf),
            (SectionKind::LineTables, &line_table_buf),
            (SectionKind::Labels, &label_buf),
            (SectionKind::ListLiterals, &list_lit_buf),
            (SectionKind::AddressPaths, &ap_buf),
            (SectionKind::LiteralPool, &literal_pool_buf),
            (SectionKind::StructShapes, &struct_shapes_buf),
            (SectionKind::EffectRows, &effect_rows_buf),
            (SectionKind::AliasTable, &alias_table_buf),
        ],
        &mut assembled,
    );

    assert_eq!(
        direct, assembled,
        "write_inkb and assemble_inkb should produce identical output"
    );

    // Also verify the assembled version can be read back.
    let mut recovered = read_inkb(&assembled).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);
}

#[test]
fn file_size_mismatch_detected() {
    let data = make_test_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    // Truncate the buffer — the file_size in the header will be larger than actual.
    buf.truncate(buf.len() - 1);

    let err = read_inkb_index(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::FileSizeMismatch { .. }),
        "expected FileSizeMismatch, got {err:?}"
    );
}

#[test]
fn bad_magic_detected() {
    let mut buf = vec![0x00; 64];
    buf[0..4].copy_from_slice(b"XYZW");

    let err = read_inkb_index(&buf).unwrap_err();
    assert!(
        matches!(err, DecodeError::BadMagic(..)),
        "expected BadMagic, got {err:?}"
    );
}

#[test]
fn roundtrip_line_entry_with_audio_ref() {
    use brink_format::{
        ContainerDef, CountingFlags, DefinitionId, DefinitionTag, LineContent, LineEntry, NameId,
        ScopeLineTable, StoryData,
    };

    let scope_id = DefinitionId::new(DefinitionTag::Address, 1);
    let data = StoryData {
        containers: vec![ContainerDef {
            id: scope_id,
            scope_id,
            name: Some(NameId(0)),
            bytecode: vec![],
            counting_flags: CountingFlags::empty(),
            path_hash: 0,
            param_count: 0,
            params: vec![],
            local: false,
        }],
        line_tables: vec![ScopeLineTable {
            scope_id,
            lines: vec![LineEntry {
                content: LineContent::Plain("Hello world\n".to_string()),
                flags: brink_format::LineFlags::from_plain("Hello world\n"),
                source_hash: 0xABCD,
                audio_ref: Some("audio/hello.wav".to_string()),
                slot_info: Vec::new(),
                source_location: None,
            }],
        }],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        address_paths: vec![],
        name_table: vec!["root".to_string()],
        list_literals: vec![],
        literal_pool: vec![],
        struct_shapes: vec![],
        private_defs: vec![],
        alias_table: vec![],
        effect_rows: vec![],
        frame_shapes: Vec::new(),
        debug_info: None,
        line_variant_groups: Vec::new(),
        source_checksum: 0,
    };

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let mut recovered = read_inkb(&buf).unwrap();
    // source_checksum is set from the binary header, not semantic data.
    recovered.source_checksum = data.source_checksum;

    assert_eq!(data, recovered);
    assert_eq!(
        recovered.line_tables[0].lines[0].audio_ref,
        Some("audio/hello.wav".to_string())
    );
}

/// `LinePart::Span` (#1716, `docs/prose-dialect-spec.md` §4.4) round-trips
/// through `.inkb` — nested, with attrs, and with a self-closing
/// (empty-children, the point-marker shape) sibling — proving the
/// `PART_SPAN` tag decodes back to the exact structure it encoded. Unlike
/// the `VAL_VEC2`/`VAL_WEIGHTED` "one-bump rule" precedent (#1519) —
/// materializing a *pre-reserved* v4 tag needs no further bump — `PART_SPAN`
/// was never reserved, so its introduction is `.inkb` `VERSION` 6's own
/// one-bump event (see `VERSION`'s doc comment in `inkb::mod`); this test
/// exercises the wire shape at that version.
#[test]
fn roundtrip_line_part_span() {
    use brink_format::{
        ContainerDef, CountingFlags, DefinitionId, DefinitionTag, LineContent, LineEntry, LinePart,
        NameId, ScopeLineTable, StoryData,
    };

    let scope_id = DefinitionId::new(DefinitionTag::Address, 1);
    let parts = vec![
        LinePart::Literal("He hands you ".to_string()),
        LinePart::Span {
            name: "item".to_string(),
            attrs: vec![("id".to_string(), "lantern".to_string())],
            children: vec![
                LinePart::Literal("the ".to_string()),
                LinePart::Span {
                    name: "b".to_string(),
                    attrs: vec![],
                    children: vec![LinePart::Literal("old".to_string())],
                },
                LinePart::Literal(" lantern".to_string()),
            ],
        },
        LinePart::Literal(". ".to_string()),
        LinePart::Span {
            name: "pause".to_string(),
            attrs: vec![],
            children: vec![],
        },
    ];
    let content = LineContent::Template(parts);
    let flags = brink_format::LineFlags::from_content(&content);

    let data = StoryData {
        containers: vec![ContainerDef {
            id: scope_id,
            scope_id,
            name: Some(NameId(0)),
            bytecode: vec![],
            counting_flags: CountingFlags::empty(),
            path_hash: 0,
            param_count: 0,
            params: vec![],
            local: false,
        }],
        line_tables: vec![ScopeLineTable {
            scope_id,
            lines: vec![LineEntry {
                content,
                flags,
                source_hash: 0xF00D,
                audio_ref: None,
                slot_info: Vec::new(),
                source_location: None,
            }],
        }],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        address_paths: vec![],
        name_table: vec!["root".to_string()],
        list_literals: vec![],
        literal_pool: vec![],
        struct_shapes: vec![],
        private_defs: vec![],
        alias_table: vec![],
        effect_rows: vec![],
        frame_shapes: Vec::new(),
        debug_info: None,
        line_variant_groups: Vec::new(),
        source_checksum: 0,
    };

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;

    assert_eq!(data, recovered);
}

// `LineFlags` is derived, not stored — `.inkb` decoding recomputes it from
// the decoded `LineContent` via `LineFlags::from_content` (see
// `decode_line_entry`). Round-trip templates whose first/last part is an
// interpolation slot (conservative — `ALL_WS` must stay unset) and an
// all-whitespace-literal template (`ALL_WS` must be set) through
// encode+decode and confirm the recomputed flags match what a fresh
// `LineFlags::from_content` call gives, i.e. flags survive the wire format,
// not just in-memory computation.
//
// `STARTS_WITH_WS`/`ENDS_WITH_WS` (previously exercised here as a #1444
// regression) were removed: they had no production consumer in
// `brink-runtime` (grep-confirmed — the only reader was `#[cfg(test)]`-only
// `OutputBuffer::ends_in_whitespace`), and the C# reference runtime's
// whitespace handling never operates at the sub-token leading/trailing
// granularity those flags encoded either, so there was no conformance gap
// behind the missing consumer.
#[test]
fn roundtrip_line_flags_for_template_content() {
    use brink_format::{
        ContainerDef, CountingFlags, DefinitionId, DefinitionTag, LineContent, LineEntry,
        LineFlags, LinePart, NameId, ScopeLineTable, StoryData,
    };

    let scope_id = DefinitionId::new(DefinitionTag::Address, 1);

    // Leading Slot alongside a whitespace-only literal — conservative:
    // the Slot's resolved content is unknown at compile time, so ALL_WS
    // must stay unset even though the only inspectable literal is
    // whitespace-only.
    let leading_slot =
        LineContent::Template(vec![LinePart::Slot(0), LinePart::Literal("  ".to_string())]);
    // Every part is a whitespace-only literal — ALL_WS must be set.
    let all_whitespace_template = LineContent::Template(vec![
        LinePart::Literal("  ".to_string()),
        LinePart::Literal("\t".to_string()),
    ]);

    let data = StoryData {
        containers: vec![ContainerDef {
            id: scope_id,
            scope_id,
            name: Some(NameId(0)),
            bytecode: vec![],
            counting_flags: CountingFlags::empty(),
            path_hash: 0,
            param_count: 0,
            params: vec![],
            local: false,
        }],
        line_tables: vec![ScopeLineTable {
            scope_id,
            lines: vec![
                LineEntry {
                    content: leading_slot.clone(),
                    flags: LineFlags::from_content(&leading_slot),
                    source_hash: 1,
                    audio_ref: None,
                    slot_info: Vec::new(),
                    source_location: None,
                },
                LineEntry {
                    content: all_whitespace_template.clone(),
                    flags: LineFlags::from_content(&all_whitespace_template),
                    source_hash: 2,
                    audio_ref: None,
                    slot_info: Vec::new(),
                    source_location: None,
                },
            ],
        }],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        address_paths: vec![],
        name_table: vec!["root".to_string()],
        list_literals: vec![],
        literal_pool: vec![],
        struct_shapes: vec![],
        private_defs: vec![],
        alias_table: vec![],
        effect_rows: vec![],
        frame_shapes: Vec::new(),
        debug_info: None,
        line_variant_groups: Vec::new(),
        source_checksum: 0,
    };

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;

    assert_eq!(data, recovered);

    let recovered_leading_slot = &recovered.line_tables[0].lines[0];
    assert!(!recovered_leading_slot.flags.contains(LineFlags::ALL_WS));

    let recovered_all_whitespace = &recovered.line_tables[0].lines[1];
    assert!(recovered_all_whitespace.flags.contains(LineFlags::ALL_WS));
}

// Regression for #954, sibling of the `.inkt` reader's guard (#745): a
// mutated `.inkb` can declare a `param_count` that disagrees with the number
// of per-param name/mode metadata entries that actually follow it. Before
// this fix, `decode_container` built the inconsistent `ContainerDef` anyway
// (violating its own documented invariant that `params.len()` always equals
// `param_count`), the same silently-invalid state the `.inkt` reader now
// rejects. The strict `.inkb` reader must reject it too, with a decode
// error, never a panic (fuzz targets exercise this exact path).
#[test]
fn container_param_count_mismatch_is_a_decode_error() {
    use brink_format::{
        ContainerDef, CountingFlags, DefinitionId, DefinitionTag, InkbIndex, NameId, ParamMeta,
        SectionEntry,
    };

    let id = DefinitionId::new(DefinitionTag::Address, 1);
    // A recognizable path_hash sentinel we can locate in the encoded bytes
    // to find the `param_count` byte that immediately follows it, without
    // hardcoding unrelated field-width assumptions (e.g. def_id encoding).
    let sentinel_path_hash: i32 = 0x7EAD_BEEF_u32.cast_signed();
    let container = ContainerDef {
        id,
        scope_id: id,
        name: None,
        bytecode: vec![],
        counting_flags: CountingFlags::empty(),
        path_hash: sentinel_path_hash,
        param_count: 1,
        params: vec![ParamMeta {
            name: NameId(0),
            is_ref: false,
        }],
        local: false,
    };

    let mut buf = Vec::new();
    write_section_containers(&[container], &mut buf);

    // Locate the sentinel path_hash's little-endian bytes; `param_count`
    // (a single byte) immediately follows it in the encoding.
    let sentinel_bytes = sentinel_path_hash.to_le_bytes();
    let sentinel_pos = buf
        .windows(4)
        .position(|w| w == sentinel_bytes)
        .expect("sentinel path_hash bytes not found in encoded container");
    let param_count_pos = sentinel_pos + 4;
    assert_eq!(buf[param_count_pos], 1, "expected param_count byte");

    // Corrupt param_count to disagree with the single ParamMeta entry that
    // follows it, producing the exact malformed shape a mutated .inkb fuzz
    // input would carry.
    buf[param_count_pos] = 0;

    let index = InkbIndex {
        version: 5,
        file_size: u32::try_from(buf.len()).unwrap(),
        checksum: 0,
        sections: vec![SectionEntry {
            kind: SectionKind::Containers,
            offset: 0,
        }],
    };

    let err = read_section_containers(&buf, &index).unwrap_err();
    assert_eq!(
        err,
        DecodeError::ParamCountMismatch {
            declared: 0,
            actual: 1,
        }
    );
}

// ── FS-3 FrameShapes section (tag 0x10) + invisible container flag ───────────

#[test]
fn roundtrip_frame_shapes_section() {
    use brink_format::{DefinitionId, DefinitionTag, FrameShapeDef, NameId};

    let mut data = i001_data();
    // Two await sites, sorted ascending by `site` as the compiler will emit.
    data.frame_shapes = vec![
        FrameShapeDef {
            site: DefinitionId::new(DefinitionTag::Address, 4),
            slots: vec![NameId(1), NameId(2)],
        },
        FrameShapeDef {
            site: DefinitionId::new(DefinitionTag::Address, 9),
            slots: vec![],
        },
    ];

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    // The optional section appears in the offset table when non-empty.
    let index = read_inkb_index(&buf).unwrap();
    assert!(
        index
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::FrameShapes),
        "FrameShapes section present when non-empty"
    );

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data.frame_shapes, recovered.frame_shapes);
    assert_eq!(data, recovered);
}

#[test]
fn frame_shapes_section_omitted_when_empty() {
    // Behind the E052 fence every compiled story has no await frame shapes, so
    // the section is omitted entirely and existing stories stay byte-identical.
    let data = i001_data();
    assert!(data.frame_shapes.is_empty());

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    assert!(
        !index
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::FrameShapes),
        "no FrameShapes section for a frame-shape-less story"
    );

    let recovered = read_inkb(&buf).unwrap();
    assert!(recovered.frame_shapes.is_empty());
}

#[test]
fn missing_frame_shapes_section_decodes_empty() {
    use brink_format::read_section_frame_shapes;

    // A buffer whose index carries no FrameShapes section reads back empty.
    let data = i001_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let index = read_inkb_index(&buf).unwrap();
    assert!(read_section_frame_shapes(&buf, &index).unwrap().is_empty());
}

#[test]
fn frame_shapes_rejects_unknown_section_version() {
    use brink_format::{
        DefinitionId, DefinitionTag, FrameShapeDef, InkbIndex, NameId, SectionEntry,
        read_section_frame_shapes, write_section_frame_shapes,
    };

    // Encode a valid section, then corrupt its leading section-local version
    // byte — the reader must reject rather than misparse.
    let real = vec![FrameShapeDef {
        site: DefinitionId::new(DefinitionTag::Address, 1),
        slots: vec![NameId(0)],
    }];
    let mut buf = Vec::new();
    write_section_frame_shapes(&real, &mut buf);
    buf[0] = 0xFF; // bogus section version

    let index = InkbIndex {
        version: 5,
        file_size: u32::try_from(buf.len()).unwrap(),
        checksum: 0,
        sections: vec![SectionEntry {
            kind: SectionKind::FrameShapes,
            offset: 0,
        }],
    };
    let err = read_section_frame_shapes(&buf, &index).unwrap_err();
    assert_eq!(
        err,
        DecodeError::UnsupportedSectionVersion {
            section: SectionKind::FrameShapes as u8,
            version: 0xFF,
        }
    );
}

// ── D6 DebugInfo section (tag 0x11, docs/debugger-spec.md §2) ───────────────

/// A small but structurally rich `DebugInfoSection`: two files (one ink, one
/// native — §2.3's per-file surface tagging), the reserved sentinel at index
/// 0 (§2.5), two containers, and one populated `DebugEntry` per container
/// exercising every field including the flag bits and a non-zero delta.
fn sample_debug_info() -> brink_format::DebugInfoSection {
    use brink_format::{
        DEBUG_FLAG_IS_STMT, DEBUG_FLAG_PROLOGUE_END, DebugContainerTable, DebugEntry,
        DebugFileEntry, DebugInfoSection, DebugLocalEntry, FileSurface,
    };

    DebugInfoSection {
        files: vec![
            DebugFileEntry {
                surface: FileSurface::Synthetic,
                path: String::new(),
                // #3261: the sentinel names no real file — zero hash, no
                // line index. Pinned so a writer that "helpfully" hashed
                // the empty string here would fail rather than pass.
                source_hash: 0,
                line_starts: Vec::new(),
            },
            DebugFileEntry {
                surface: FileSurface::Ink,
                path: "story.ink".to_string(),
                source_hash: 0x0123_4567_89ab_cdef,
                // Deliberately irregular gaps: a delta encoder that assumed
                // a fixed stride, or dropped the leading 0, round-trips
                // wrong here rather than accidentally right.
                line_starts: vec![0, 11, 12, 47, 130],
            },
            DebugFileEntry {
                surface: FileSurface::Native,
                path: "scene.brink".to_string(),
                // A distinct hash from the entry above, so a round trip
                // that collapsed both files onto one value fails.
                source_hash: 0xfedc_ba98_7654_3210,
                line_starts: vec![0, 5],
            },
        ],
        containers: vec![
            DebugContainerTable {
                entries: vec![
                    DebugEntry {
                        bytecode_offset: 0,
                        file_idx: 1,
                        range_start: 10,
                        range_len: 5,
                        kind_token: 0x0001_0002,
                        flags: DEBUG_FLAG_IS_STMT | DEBUG_FLAG_PROLOGUE_END,
                    },
                    DebugEntry {
                        bytecode_offset: 4,
                        file_idx: 1,
                        range_start: 20,
                        range_len: 3,
                        kind_token: 0x0001_0003,
                        flags: DEBUG_FLAG_IS_STMT,
                    },
                ],
                // D7's locals rows, every flag combination the section
                // version 2 flags byte can carry (#3185, #3395): a
                // parameter (no range), an authored temp (range), a
                // synthetic temp with and without a range.
                locals: vec![
                    DebugLocalEntry {
                        slot: 0,
                        name: "n".to_string(),
                        declaring_range: None,
                        synthetic: false,
                    },
                    DebugLocalEntry {
                        slot: 1,
                        name: "doubled".to_string(),
                        declaring_range: Some((1, 10, 5)),
                        synthetic: false,
                    },
                    DebugLocalEntry {
                        slot: 2,
                        name: "$lift0".to_string(),
                        declaring_range: Some((1, 20, 3)),
                        synthetic: true,
                    },
                    DebugLocalEntry {
                        slot: 3,
                        name: "$lift1".to_string(),
                        declaring_range: None,
                        synthetic: true,
                    },
                ],
            },
            DebugContainerTable {
                entries: vec![DebugEntry {
                    bytecode_offset: 0,
                    file_idx: 2,
                    range_start: 0,
                    range_len: 8,
                    kind_token: 0x0000_002C,
                    flags: DEBUG_FLAG_IS_STMT | DEBUG_FLAG_PROLOGUE_END,
                }],
                locals: Vec::new(),
            },
        ],
    }
}

#[test]
fn roundtrip_debug_info_section() {
    let mut data = i001_data();
    data.debug_info = Some(sample_debug_info());

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    assert!(
        index
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::DebugInfo),
        "DebugInfo section present when Some"
    );

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data.debug_info, recovered.debug_info);
    assert_eq!(data, recovered);
}

/// The same table through the `.inkt` text form (`(local slot "name"
/// synthetic? (range …)?)`), so the #3395 `synthetic` marker and its
/// absence both survive the round trip the way the binary flags byte does.
#[test]
fn roundtrip_debug_info_section_inkt() {
    let mut data = i001_data();
    data.debug_info = Some(sample_debug_info());

    let mut text = String::new();
    brink_format::write_inkt(&data, &mut text).unwrap();
    assert!(
        text.contains("(local 2 \"$lift0\" synthetic (range 1 20 3))"),
        "synthetic marker precedes the range: {text}"
    );
    assert!(text.contains("(local 3 \"$lift1\" synthetic)"));
    assert!(text.contains("(local 1 \"doubled\" (range 1 10 5))"));

    let recovered = brink_format::read_inkt(&text).unwrap();
    assert_eq!(data.debug_info, recovered.debug_info);
}

#[test]
fn debug_info_section_omitted_when_none() {
    // The ship-policy default (§1.2): a story compiled without the debug
    // flag carries `debug_info: None`, and the section is omitted entirely
    // — existing stories stay byte-identical, no `VERSION` bump.
    let data = i001_data();
    assert!(data.debug_info.is_none());

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    assert!(
        !index
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::DebugInfo),
        "no DebugInfo section for a story compiled without the debug flag"
    );

    let recovered = read_inkb(&buf).unwrap();
    assert!(recovered.debug_info.is_none());
}

#[test]
fn missing_debug_info_section_decodes_none() {
    use brink_format::read_section_debug_info;

    // A buffer whose index carries no DebugInfo section reads back `None`,
    // not an empty-but-`Some` section — see `StoryData::debug_info`'s doc
    // for why `Option` (not emptiness) is this section's "requested" signal.
    let data = i001_data();
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let index = read_inkb_index(&buf).unwrap();
    assert!(read_section_debug_info(&buf, &index).unwrap().is_none());
}

#[test]
fn debug_info_rejects_unknown_section_version() {
    use brink_format::{
        InkbIndex, SectionEntry, read_section_debug_info, write_section_debug_info,
    };

    let mut buf = Vec::new();
    write_section_debug_info(&sample_debug_info(), &mut buf);
    buf[0] = 0xFF; // bogus section-local version

    let index = InkbIndex {
        version: 5,
        file_size: u32::try_from(buf.len()).unwrap(),
        checksum: 0,
        sections: vec![SectionEntry {
            kind: SectionKind::DebugInfo,
            offset: 0,
        }],
    };
    let err = read_section_debug_info(&buf, &index).unwrap_err();
    assert_eq!(
        err,
        DecodeError::UnsupportedSectionVersion {
            section: SectionKind::DebugInfo as u8,
            version: 0xFF,
        }
    );
}

/// #3395: the locals row's flags byte (section version 2 — bit 0 has-range,
/// bit 1 synthetic) is STRICT, unlike the entry `flags` byte below: a
/// reserved bit set is a decode error naming the byte, the same discipline
/// `DirectEffects`' extension-flags byte follows. Finds the byte by
/// re-encoding a one-row table and patching the sole flags position.
#[test]
fn debug_info_rejects_reserved_local_flag_bits() {
    use brink_format::{
        DebugContainerTable, DebugLocalEntry, read_section_debug_info, write_section_debug_info,
    };

    let mut section = sample_debug_info();
    section.containers = vec![DebugContainerTable {
        entries: Vec::new(),
        locals: vec![DebugLocalEntry {
            slot: 7,
            name: "x".to_string(),
            declaring_range: None,
            synthetic: true,
        }],
    }];
    let mut buf = Vec::new();
    write_section_debug_info(&section, &mut buf);
    // The row is the last thing written: slot u16, name (u32 len + "x"),
    // flags u8 — so the flags byte is the final byte of the buffer.
    let flags_at = buf.len() - 1;
    assert_eq!(buf[flags_at], 0b10, "synthetic, no range");

    let index = debug_info_index_for(&buf);
    let ok = read_section_debug_info(&buf, &index).unwrap().unwrap();
    assert_eq!(ok.containers[0].locals, section.containers[0].locals);

    buf[flags_at] = 0b110;
    let err = read_section_debug_info(&buf, &index).unwrap_err();
    assert_eq!(err, DecodeError::InvalidDebugLocalFlags(0b110));
}

/// §2.2's ruled, explicit departure from this format's default
/// strict-rejection posture: a `DebugInfo` reader MUST NOT reject an entry
/// solely because a reserved `flags` bit is set. Round-tripping an entry
/// with every reserved bit (2-7) set must preserve those bits exactly, not
/// mask or reject them.
#[test]
fn debug_info_reader_tolerates_reserved_flag_bits() {
    use brink_format::{
        DEBUG_FLAG_IS_STMT, DEBUG_FLAG_RESERVED_MASK, DebugContainerTable, DebugEntry,
        read_section_debug_info, write_section_debug_info,
    };

    let mut section = sample_debug_info();
    // Overwrite with a single container/entry carrying every reserved bit.
    section.containers = vec![DebugContainerTable {
        entries: vec![DebugEntry {
            bytecode_offset: 0,
            file_idx: 0,
            range_start: 0,
            range_len: 1,
            kind_token: 0,
            flags: DEBUG_FLAG_IS_STMT | DEBUG_FLAG_RESERVED_MASK,
        }],
        locals: Vec::new(),
    }];

    let mut buf = Vec::new();
    write_section_debug_info(&section, &mut buf);
    let index = debug_info_index_for(&buf);
    let recovered = read_section_debug_info(&buf, &index).unwrap().unwrap();
    assert_eq!(
        recovered.containers[0].entries[0].flags,
        DEBUG_FLAG_IS_STMT | DEBUG_FLAG_RESERVED_MASK,
        "reserved bits must round-trip unmodified, not be masked or rejected"
    );
}

/// Local helper: wrap a raw `write_section_debug_info` buffer (no header
/// framing) in the minimal `InkbIndex` `read_section_debug_info` needs —
/// mirrors `debug_info_rejects_unknown_section_version`'s inline
/// construction, factored out since two tests need it now.
fn debug_info_index_for(buf: &[u8]) -> brink_format::InkbIndex {
    brink_format::InkbIndex {
        version: 5,
        file_size: u32::try_from(buf.len()).unwrap(),
        checksum: 0,
        sections: vec![brink_format::SectionEntry {
            kind: SectionKind::DebugInfo,
            offset: 0,
        }],
    }
}

/// Documents actual current behavior against a claim in
/// `docs/debugger-spec.md` §1.1 ("the 2026-07-27 format durability doctrine
/// made unknown sections skippable-by-length") — verified here, not merely
/// cited. The skip mechanism that doctrine promises (`SectionEntry` gaining
/// a per-entry LENGTH, `docs/decision-log.md` "Format durability doctrine:
/// strictness follows durability; sections skippable") is issue #1519,
/// still **OPEN** per `docs/ruling-ledger.md` ("`SectionEntry` has **no
/// length**"). `read_inkb_index` calls `SectionKind::from_u8(kind_tag)?`
/// unconditionally for every offset-table entry, so ANY unrecognized tag —
/// this test uses the current true frontier, `0x13`, one past
/// `LineVariantGroups` (#3273; previously `0x12`, one past `DebugInfo`) —
/// fails the entire file's index parse (Tier 2), not just that one section.
/// A reader that predates `SectionKind::DebugInfo` therefore fails CLOSED
/// on a debug-info-bearing `.inkb` today, not skip-harmlessly: #1519 is the
/// prerequisite that would make it actually skippable, and is out of D6's
/// scope (a separate, larger ticket) — flagged on issue #3184 rather than
/// silently building a workaround here.
#[test]
fn unrecognized_section_kind_hard_rejects_the_whole_file_pending_1519() {
    let mut data = i001_data();
    data.debug_info = Some(sample_debug_info());
    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    let debug_pos = index
        .sections
        .iter()
        .position(|s| s.kind == SectionKind::DebugInfo)
        .expect("DebugInfo section present");
    // Header layout, per `crates/internal/brink-format/src/inkb/mod.rs`'s
    // own module doc: a 16-byte preamble, then 8 bytes per offset-table
    // entry with the kind tag as that entry's first byte.
    let kind_byte_offset = 16 + debug_pos * 8;
    assert_eq!(buf[kind_byte_offset], SectionKind::DebugInfo as u8);
    buf[kind_byte_offset] = 0x13; // one past the current real frontier

    let err = read_inkb_index(&buf).unwrap_err();
    assert_eq!(
        err,
        DecodeError::InvalidSectionKind(0x13),
        "an unrecognized SectionKind tag fails the WHOLE file's index parse \
         today (pending #1519), not just the one unknown section"
    );
}

#[test]
fn roundtrip_invisible_container_flag() {
    use brink_format::CountingFlags;

    let mut data = i001_data();
    assert!(!data.containers.is_empty());
    // Mark a container invisible (the synthesized-continuation marker, §11.2).
    data.containers[0].counting_flags |= CountingFlags::INVISIBLE;

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert!(
        recovered.containers[0]
            .counting_flags
            .contains(CountingFlags::INVISIBLE),
        "INVISIBLE flag survives the .inkb round-trip"
    );
    assert_eq!(data, recovered);
}

/// NS-A8 (`docs/tower-mini-spec.md` T5): first emission of the
/// `VAL_VEC2`..`VAL_MAT4` wire tags — write→read identity for tower-valued
/// globals, bare and nested inside a collection. The wire is explicit
/// little-endian f32 lanes (vec/quat `x, y(, z, w)`; matrices column-major
/// column-by-column), never glam's memory layout.
#[test]
fn roundtrip_tower_valued_globals() {
    use brink_format::{DefinitionId, DefinitionTag, GlobalVarDef, NameId, Value, ValueType};

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;

    let towers = [
        (ValueType::Vec2, Value::Vec2(glam::Vec2::new(1.5, -2.25))),
        (
            ValueType::Vec3,
            Value::Vec3(glam::Vec3::new(0.0, -0.0, f32::MAX)),
        ),
        (
            ValueType::Vec4,
            Value::Vec4(glam::Vec4::new(1.0, 2.0, 3.0, 4.0)),
        ),
        (
            ValueType::Quat,
            Value::Quat(glam::Quat::from_xyzw(0.5, -0.5, 0.5, 0.5)),
        ),
        (
            ValueType::Mat2,
            Value::Mat2(glam::Mat2::from_cols_array(&[1.0, 2.0, 3.0, 4.0])),
        ),
        (
            ValueType::Mat3,
            Value::Mat3(glam::Mat3::from_cols_array(&[
                1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0,
            ])),
        ),
        (
            ValueType::Mat4,
            Value::Mat4(glam::Mat4::from_cols_array(&[
                1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0,
                16.0,
            ])),
        ),
    ];
    for (i, (vt, v)) in towers.iter().enumerate() {
        data.variables.push(GlobalVarDef {
            id: DefinitionId::new(DefinitionTag::GlobalVar, next_id + i as u64),
            name: NameId(0),
            value_type: *vt,
            default_value: v.clone(),
            mutable: true,
            local: false,
        });
    }
    // Nested inside a collection, like the handle/projection tests above.
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id + 7),
        name: NameId(0),
        value_type: ValueType::Array,
        default_value: Value::array(vec![
            Value::Vec2(glam::Vec2::ONE),
            Value::some(Value::Vec3(glam::Vec3::Z)),
        ]),
        mutable: true,
        local: false,
    });

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data, recovered);
}

/// NS-A8 (`docs/tower-mini-spec.md` T4/T5): a NaN lane must cross the wire
/// bit-for-bit even though the value no longer compares equal to itself —
/// compared here by lane *bits*, not `PartialEq` (which correctly reads
/// `false` for a NaN-bearing vector).
#[test]
fn tower_nan_lane_crosses_the_wire_bit_exact() {
    use brink_format::{DefinitionId, DefinitionTag, GlobalVarDef, NameId, Value, ValueType};

    let mut data = i001_data();
    let next_id = data.variables.len() as u64;
    let lanes = [f32::NAN, f32::NEG_INFINITY, -0.0];
    data.variables.push(GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, next_id),
        name: NameId(0),
        value_type: ValueType::Vec3,
        default_value: Value::Vec3(glam::Vec3::from_array(lanes)),
        mutable: true,
        local: false,
    });

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);
    let recovered = read_inkb(&buf).unwrap();

    let recovered_value = &recovered
        .variables
        .last()
        .expect("tower global present")
        .default_value;
    let Value::Vec3(v) = recovered_value else {
        unreachable!("expected Vec3, got {recovered_value:?}");
    };
    let got = v.to_array();
    for (i, lane) in lanes.iter().enumerate() {
        assert_eq!(
            lane.to_bits(),
            got[i].to_bits(),
            "lane {i} drifted: {lane} -> {}",
            got[i]
        );
    }
}

/// #3273 stage 1: a `LineVariantGroups` section round-trips through
/// `.inkb` — the record that ties `dims.product()` consecutive line-table
/// entries back to one authored line with enumerated alternatives.
#[test]
fn roundtrip_line_variant_groups_section() {
    let mut data = i001_data();
    let scope_id = data.line_tables[0].scope_id;
    data.line_variant_groups = vec![
        brink_format::LineVariantGroup {
            scope_id,
            base: 0,
            dims: vec![2, 3],
        },
        brink_format::LineVariantGroup {
            scope_id,
            base: 6,
            dims: vec![4],
        },
    ];

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    assert!(
        index
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::LineVariantGroups),
        "LineVariantGroups section present when non-empty"
    );

    let mut recovered = read_inkb(&buf).unwrap();
    recovered.source_checksum = data.source_checksum;
    assert_eq!(data.line_variant_groups, recovered.line_variant_groups);
    assert_eq!(data, recovered);
}

/// The section is omitted entirely when empty — every story without
/// variant groups stays byte-identical to before the section existed
/// (the `Visibility`/`FrameShapes`/`DebugInfo` additive pattern).
#[test]
fn line_variant_groups_section_omitted_when_empty() {
    let data = i001_data();
    assert!(data.line_variant_groups.is_empty());

    let mut buf = Vec::new();
    write_inkb(&data, &mut buf);

    let index = read_inkb_index(&buf).unwrap();
    assert!(
        !index
            .sections
            .iter()
            .any(|s| s.kind == SectionKind::LineVariantGroups),
        "no LineVariantGroups section for a story without variant groups"
    );

    let recovered = read_inkb(&buf).unwrap();
    assert!(recovered.line_variant_groups.is_empty());
}
