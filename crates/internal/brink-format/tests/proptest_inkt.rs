#![cfg(feature = "inkt")]
#![allow(clippy::unwrap_used)]

use brink_format::{
    ContainerDef, CountingFlags, DefinitionId, DefinitionTag, ExternalFnDef, GlobalVarDef,
    LineContent, LineEntry, LinePart, ListDef, ListItemDef, ListValue, NameId, Opcode,
    PluralCategory, ScopeLineTable, SelectKey, SlotInfo, SourceLocation, StoryData, Value,
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

fn arb_value() -> impl Strategy<Value = Value> {
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
    ]
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
    )
        .prop_map(|(id, bytecode, counting_flags, lines)| {
            let def = ContainerDef {
                id,
                scope_id: id,
                name: None,
                bytecode,
                counting_flags,
                path_hash: 0,
            };
            let lt = ScopeLineTable {
                scope_id: id,
                lines,
            };
            (def, lt)
        })
}

/// Generate a global var with consistent `value_type` and `default_value`.
fn arb_global_var() -> impl Strategy<Value = GlobalVarDef> {
    (arb_def_id(), arb_name_id(), arb_value(), any::<bool>()).prop_map(
        |(id, name, default_value, mutable)| {
            let value_type = default_value.value_type();
            GlobalVarDef {
                id,
                name,
                value_type,
                default_value,
                mutable,
            }
        },
    )
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

fn arb_story_data() -> impl Strategy<Value = StoryData> {
    (
        prop::collection::vec(arb_container_with_lines(), 0..5),
        prop::collection::vec(arb_global_var(), 0..5),
        prop::collection::vec(arb_list_def(), 0..5),
        prop::collection::vec(arb_list_item(), 0..5),
        prop::collection::vec(arb_external(), 0..5),
        prop::collection::vec("[^\"\\\\\x00]*", 0..8),
        any::<u32>(),
    )
        .prop_map(
            |(pairs, variables, list_defs, list_items, externals, name_table, source_checksum)| {
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
                    name_table,
                    list_literals: vec![],
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
