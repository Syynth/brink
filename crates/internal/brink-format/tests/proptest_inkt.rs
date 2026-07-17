#![cfg(feature = "inkt")]
#![allow(clippy::unwrap_used)]

use brink_format::{
    AddressPath, ClosureEnvEntry, ContainerDef, CountingFlags, DefinitionId, DefinitionTag,
    ExternalFnDef, GlobalVarDef, LineContent, LineEntry, LinePart, ListDef, ListItemDef, ListValue,
    MapKey, NameId, Opcode, OrderedMap, PluralCategory, ProjSegment, ScopeLineTable, SelectKey,
    ShapeId, SlotInfo, SourceLocation, StoryData, Value,
};
use proptest::prelude::*;

// ── Strategies ──────────────────────────────────────────────────────────────

fn arb_tag() -> impl Strategy<Value = DefinitionTag> {
    prop_oneof![
        Just(DefinitionTag::Address),
        Just(DefinitionTag::GlobalVar),
        Just(DefinitionTag::ListDef),
        Just(DefinitionTag::ListItem),
        Just(DefinitionTag::ExternalFn),
    ]
}

fn arb_def_id() -> impl Strategy<Value = DefinitionId> {
    (arb_tag(), any::<u64>()).prop_map(|(tag, hash)| DefinitionId::new(tag, hash))
}

fn arb_name_id() -> impl Strategy<Value = NameId> {
    any::<u16>().prop_map(NameId)
}

fn arb_plural_category() -> impl Strategy<Value = PluralCategory> {
    prop_oneof![
        Just(PluralCategory::Zero),
        Just(PluralCategory::One),
        Just(PluralCategory::Two),
        Just(PluralCategory::Few),
        Just(PluralCategory::Many),
        Just(PluralCategory::Other),
    ]
}

fn arb_select_key() -> impl Strategy<Value = SelectKey> {
    prop_oneof![
        arb_plural_category().prop_map(SelectKey::Cardinal),
        arb_plural_category().prop_map(SelectKey::Ordinal),
        any::<i32>().prop_map(SelectKey::Exact),
        "[a-zA-Z_][a-zA-Z0-9_]*".prop_map(SelectKey::Keyword),
    ]
}

fn arb_line_part() -> impl Strategy<Value = LinePart> {
    prop_oneof![
        "[^\"\\\\\x00]*".prop_map(LinePart::Literal),
        any::<u8>().prop_map(LinePart::Slot),
        (
            any::<u8>(),
            prop::collection::vec((arb_select_key(), "[^\"\\\\\x00]*"), 0..3),
            "[^\"\\\\\x00]*",
        )
            .prop_map(|(slot, variants, default)| LinePart::Select {
                slot,
                variants,
                default,
            }),
    ]
}

fn arb_line_content() -> impl Strategy<Value = LineContent> {
    prop_oneof![
        "[^\"\\\\\x00]*".prop_map(LineContent::Plain),
        prop::collection::vec(arb_line_part(), 1..4).prop_map(LineContent::Template),
    ]
}

fn arb_slot_info() -> impl Strategy<Value = SlotInfo> {
    (any::<u8>(), "[a-zA-Z_][a-zA-Z0-9_.]{0,20}").prop_map(|(index, name)| SlotInfo { index, name })
}

fn arb_source_location() -> impl Strategy<Value = SourceLocation> {
    ("[a-zA-Z0-9/_.-]{1,30}", any::<u32>(), any::<u32>()).prop_map(
        |(file, range_start, range_end)| SourceLocation {
            file,
            range_start,
            range_end,
        },
    )
}

fn arb_line_entry() -> impl Strategy<Value = LineEntry> {
    (
        arb_line_content(),
        any::<u64>(),
        prop::option::of("[a-z0-9/_-]{1,20}".prop_map(String::from)),
        prop::collection::vec(arb_slot_info(), 0..3),
        prop::option::of(arb_source_location()),
    )
        .prop_map(
            |(content, source_hash, audio_ref, slot_info, source_location)| {
                let flags = brink_format::LineFlags::from_content(&content);
                LineEntry {
                    content,
                    flags,
                    source_hash,
                    audio_ref,
                    slot_info,
                    source_location,
                }
            },
        )
}

fn arb_counting_flags() -> impl Strategy<Value = CountingFlags> {
    (0u8..8).prop_map(CountingFlags::from_bits_truncate)
}

/// Generate a representable f32 that roundtrips through Display/parse.
fn arb_inkt_float() -> impl Strategy<Value = f32> {
    prop_oneof![
        Just(0.0f32),
        Just(1.0f32),
        Just(-1.0f32),
        Just(0.5f32),
        Just(3.125f32),
        (-1000.0f32..1000.0f32),
    ]
}

fn arb_map_key() -> impl Strategy<Value = MapKey> {
    prop_oneof![
        any::<i32>().prop_map(MapKey::Int),
        "[^\"\\\\\x00]*".prop_map(|s: String| MapKey::Str(s.into())),
        any::<bool>().prop_map(MapKey::Bool),
    ]
}

fn arb_shape_id() -> impl Strategy<Value = ShapeId> {
    any::<u32>().prop_map(ShapeId)
}

/// Leaf values (never recurse) — mirrors `proptest_inkb.rs`'s split.
/// `FnRef` has no nested payload so it lives here alongside the other
/// leaves; `Record`/`Closure` recurse into their fields/env and are added
/// below in `arb_value`'s `prop_recursive` block (issue #742: the `.inkt`
/// grammar previously had no `record_value`/`fn_ref_value`/`closure_value`
/// rule, so `write_inkt` could emit text `read_inkt` could not parse back —
/// fixed alongside this coverage). `Handle` is also a leaf: T1d's `.inkt`
/// atom (`handle_value` in the grammar) lands its reader in the same PR as
/// its writer, so it never joins the `#742` exclusion class.
fn arb_value_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i32>().prop_map(Value::Int),
        arb_inkt_float().prop_map(Value::Float),
        any::<bool>().prop_map(Value::Bool),
        "[^\"\\\\\x00]*".prop_map(|s: String| Value::String(s.into())),
        arb_def_id().prop_map(Value::DivertTarget),
        Just(Value::Null),
        (
            prop::collection::vec(arb_def_id(), 0..3),
            prop::collection::vec(arb_def_id(), 0..3),
        )
            .prop_map(|(items, origins)| Value::List(ListValue { items, origins }.into())),
        (arb_name_id(), any::<u64>()).prop_map(|(kind, id)| Value::handle(kind, id)),
        arb_def_id().prop_map(Value::FnRef),
    ]
}

/// One projection segment: `Index` (leaf) or `Key` (nests an arbitrary
/// `Value` at bounded depth, e.g. a struct-field-name string or a non-`Int`
/// map key).
fn arb_proj_segment(
    inner: impl Strategy<Value = Value> + Clone,
) -> impl Strategy<Value = ProjSegment> {
    prop_oneof![
        any::<i32>().prop_map(ProjSegment::Index),
        inner.prop_map(ProjSegment::Key),
    ]
}

/// `Array`/`Map`/`Record`/`Closure`/`Projection` (value-model-spec §4,
/// t1c-spec §1/§6, t1e-spec §3) nested to a bounded depth — the `.inkt`
/// reader's `array_value`/`map_value`/`record_value`/`closure_value`/
/// `projection_value` rules support all five, so this closes the
/// "value -> inkt text -> value == identity" law (issue #672 workstream B
/// item 2, extended by #742/#871 to the T1c function-value and T1e
/// projection tags) for every recursive `Value` variant, not just scalars.
/// `Projection` was the one member of this family issue #871 found still
/// missing coverage here — its reader (`parse_projection_value`) already
/// existed, but nothing had ever generated a `Value::Projection` to prove it.
fn arb_value() -> impl Strategy<Value = Value> {
    arb_value_leaf().prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::array),
            prop::collection::vec((arb_map_key(), inner.clone()), 0..4).prop_map(|entries| {
                let mut map = OrderedMap::new();
                for (key, value) in entries {
                    map.insert(key, value);
                }
                Value::map(map)
            }),
            (arb_shape_id(), prop::collection::vec(inner.clone(), 0..4))
                .prop_map(|(shape, fields)| Value::record(shape, fields)),
            (
                arb_def_id(),
                prop::collection::vec(arb_proj_segment(inner.clone()), 0..3),
            )
                .prop_map(|(cell, segments)| Value::projection(cell, segments)),
            (
                arb_def_id(),
                prop::collection::vec((arb_name_id(), any::<bool>(), inner), 0..3),
            )
                .prop_map(|(target, raw_env)| {
                    let env = raw_env
                        .into_iter()
                        .map(|(name, is_ref, payload)| ClosureEnvEntry {
                            name,
                            is_ref,
                            payload,
                        })
                        .collect();
                    Value::closure(target, env)
                }),
        ]
    })
}

/// Generate valid opcodes (not random bytes).
fn arb_opcode() -> impl Strategy<Value = Opcode> {
    prop_oneof![
        any::<i32>().prop_map(Opcode::PushInt),
        arb_inkt_float().prop_map(Opcode::PushFloat),
        any::<bool>().prop_map(Opcode::PushBool),
        any::<u16>().prop_map(Opcode::PushString),
        any::<u16>().prop_map(Opcode::PushList),
        arb_def_id().prop_map(Opcode::PushDivertTarget),
        Just(Opcode::PushNull),
        Just(Opcode::Pop),
        Just(Opcode::Duplicate),
        Just(Opcode::Add),
        Just(Opcode::Subtract),
        Just(Opcode::Multiply),
        Just(Opcode::Divide),
        Just(Opcode::Modulo),
        Just(Opcode::Negate),
        Just(Opcode::Equal),
        Just(Opcode::NotEqual),
        Just(Opcode::Not),
        Just(Opcode::And),
        Just(Opcode::Or),
        arb_def_id().prop_map(Opcode::GetGlobal),
        arb_def_id().prop_map(Opcode::SetGlobal),
        any::<u16>().prop_map(Opcode::DeclareTemp),
        any::<u16>().prop_map(Opcode::GetTemp),
        any::<u16>().prop_map(Opcode::SetTemp),
        any::<i32>().prop_map(Opcode::Jump),
        any::<i32>().prop_map(Opcode::JumpIfFalse),
        arb_def_id().prop_map(Opcode::Goto),
        Just(Opcode::GotoVariable),
        arb_def_id().prop_map(Opcode::EnterContainer),
        Just(Opcode::ExitContainer),
        arb_def_id().prop_map(Opcode::Call),
        Just(Opcode::Return),
        (any::<u16>(), any::<u8>()).prop_map(|(idx, slots)| Opcode::EmitLine(idx, slots)),
        (any::<u16>(), any::<u8>()).prop_map(|(idx, slots)| Opcode::EvalLine(idx, slots)),
        Just(Opcode::EmitValue),
        Just(Opcode::EmitNewline),
        Just(Opcode::Glue),
        Just(Opcode::Done),
        Just(Opcode::End),
        Just(Opcode::Nop),
        // Records (TM-4, issue #871: the `.inkt` reader had no case for any
        // of these mnemonics, so a container emitting them failed to
        // round-trip).
        any::<u32>().prop_map(Opcode::RecordNew),
        any::<u16>().prop_map(Opcode::RecordGetDyn),
        any::<u16>().prop_map(Opcode::RecordSetDyn),
        any::<u16>().prop_map(Opcode::RecordGet),
        any::<u16>().prop_map(Opcode::RecordSet),
        // Conversion intrinsics (TM-3 completion, issue #871).
        Just(Opcode::ConvertInt),
        Just(Opcode::ConvertFloat),
        Just(Opcode::ConvertString),
        // Function values (T1c, issue #871).
        arb_def_id().prop_map(Opcode::PushFnRef),
        (arb_def_id(), any::<u8>()).prop_map(|(target, bound_count)| Opcode::MakeClosure {
            target,
            bound_count,
        }),
        any::<u8>().prop_map(Opcode::CallValue),
        any::<u8>().prop_map(Opcode::BindValue),
        // Path projections (T1e, issue #871).
        (arb_def_id(), any::<u8>()).prop_map(|(root, segment_count)| Opcode::MakeProjection {
            root,
            segment_count,
        }),
        Just(Opcode::ProjRead),
        Just(Opcode::ProjWrite),
    ]
}

fn arb_bytecode() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(arb_opcode(), 0..8).prop_map(|ops| {
        let mut buf = Vec::new();
        for op in &ops {
            op.encode(&mut buf);
        }
        buf
    })
}

fn arb_container_with_lines() -> impl Strategy<Value = (ContainerDef, ScopeLineTable)> {
    (
        arb_def_id(),
        arb_bytecode(),
        arb_counting_flags(),
        prop::collection::vec(arb_line_entry(), 0..4),
        any::<u8>(),
        any::<bool>(),
    )
        .prop_map(
            |(id, bytecode, counting_flags, lines, param_count, local)| {
                let def = ContainerDef {
                    id,
                    scope_id: id,
                    name: None,
                    bytecode,
                    counting_flags,
                    path_hash: 0,
                    param_count,
                    params: Vec::new(),
                    local,
                };
                let lt = ScopeLineTable {
                    scope_id: id,
                    lines,
                };
                (def, lt)
            },
        )
}

/// Generate a global var with consistent `value_type` and `default_value`.
fn arb_global_var() -> impl Strategy<Value = GlobalVarDef> {
    (
        arb_def_id(),
        arb_name_id(),
        arb_value(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(|(id, name, default_value, mutable, local)| {
            let value_type = default_value.value_type();
            GlobalVarDef {
                id,
                name,
                value_type,
                default_value,
                mutable,
                local,
            }
        })
}

fn arb_list_def() -> impl Strategy<Value = ListDef> {
    (
        arb_def_id(),
        arb_name_id(),
        prop::collection::vec((arb_name_id(), any::<i32>()), 0..5),
    )
        .prop_map(|(id, name, items)| ListDef { id, name, items })
}

fn arb_list_item() -> impl Strategy<Value = ListItemDef> {
    (arb_def_id(), arb_def_id(), any::<i32>(), arb_name_id()).prop_map(
        |(id, origin, ordinal, name)| ListItemDef {
            id,
            origin,
            ordinal,
            name,
        },
    )
}

fn arb_external() -> impl Strategy<Value = ExternalFnDef> {
    (
        arb_def_id(),
        arb_name_id(),
        any::<u8>(),
        prop::option::of(arb_def_id()),
    )
        .prop_map(|(id, name, arg_count, fallback)| ExternalFnDef {
            id,
            name,
            arg_count,
            fallback,
        })
}

fn arb_address_path() -> impl Strategy<Value = AddressPath> {
    (arb_name_id(), arb_def_id()).prop_map(|(path, target)| AddressPath { path, target })
}

fn arb_story_data() -> impl Strategy<Value = StoryData> {
    (
        prop::collection::vec(arb_container_with_lines(), 0..5),
        prop::collection::vec(arb_global_var(), 0..5),
        prop::collection::vec(arb_list_def(), 0..5),
        prop::collection::vec(arb_list_item(), 0..5),
        prop::collection::vec(arb_external(), 0..5),
        prop::collection::vec(arb_address_path(), 0..5),
        prop::collection::vec("[^\"\\\\\x00]*", 0..8),
        any::<u32>(),
    )
        .prop_map(
            |(
                pairs,
                variables,
                list_defs,
                list_items,
                externals,
                address_paths,
                name_table,
                source_checksum,
            )| {
                let (containers, mut line_tables): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
                // Sort line tables by scope_id to match reader's output ordering.
                line_tables.sort_by_key(|lt| lt.scope_id.to_raw());
                StoryData {
                    containers,
                    line_tables,
                    variables,
                    list_defs,
                    list_items,
                    externals,
                    addresses: vec![],
                    address_paths,
                    name_table,
                    list_literals: vec![],
                    literal_pool: vec![],
                    struct_shapes: vec![],
                    private_defs: vec![],
                    alias_table: vec![],
                    source_checksum,
                }
            },
        )
}

// ── Tests ───────────────────────────────────────────────────────────────────

proptest! {
    /// Write-then-read is a perfect round-trip for all StoryData values.
    #[test]
    fn write_read_inkt_roundtrip(story in arb_story_data()) {
        let mut buf = String::new();
        brink_format::write_inkt(&story, &mut buf).unwrap();

        let recovered = brink_format::read_inkt(&buf).unwrap();
        prop_assert_eq!(story, recovered);
    }
}
