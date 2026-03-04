#![allow(clippy::unwrap_used)]

use brink_format::{
    ContainerDef, ContainerLineTable, CountingFlags, DefinitionId, DefinitionTag, ExternalFnDef,
    GlobalVarDef, LineContent, LineEntry, LinePart, ListDef, ListItemDef, NameId, PluralCategory,
    SectionKind, SelectKey, StoryData, Value, ValueType, read_inkb, read_inkb_index, write_inkb,
};
use proptest::prelude::*;

// ── Strategies ──────────────────────────────────────────────────────────────

fn arb_tag() -> impl Strategy<Value = DefinitionTag> {
    prop_oneof![
        Just(DefinitionTag::Container),
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
        ".*".prop_map(SelectKey::Keyword),
    ]
}

fn arb_line_part() -> impl Strategy<Value = LinePart> {
    prop_oneof![
        ".*".prop_map(LinePart::Literal),
        any::<u8>().prop_map(LinePart::Slot),
        (
            any::<u8>(),
            prop::collection::vec((arb_select_key(), ".*"), 0..3),
            ".*",
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
        ".*".prop_map(LineContent::Plain),
        prop::collection::vec(arb_line_part(), 1..4).prop_map(LineContent::Template),
    ]
}

fn arb_line_entry() -> impl Strategy<Value = LineEntry> {
    (arb_line_content(), any::<u64>()).prop_map(|(content, source_hash)| LineEntry {
        content,
        source_hash,
    })
}

fn arb_counting_flags() -> impl Strategy<Value = CountingFlags> {
    (0u8..8).prop_map(CountingFlags::from_bits_truncate)
}

fn arb_value_type() -> impl Strategy<Value = ValueType> {
    prop_oneof![
        Just(ValueType::Int),
        Just(ValueType::Float),
        Just(ValueType::Bool),
        Just(ValueType::String),
        Just(ValueType::DivertTarget),
        Just(ValueType::Null),
    ]
}

fn arb_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i32>().prop_map(Value::Int),
        any::<f32>().prop_map(Value::Float),
        any::<bool>().prop_map(Value::Bool),
        ".*".prop_map(Value::String),
        arb_def_id().prop_map(Value::DivertTarget),
        Just(Value::Null),
    ]
}

fn arb_container_with_lines() -> impl Strategy<Value = (ContainerDef, ContainerLineTable)> {
    (
        arb_def_id(),
        prop::collection::vec(any::<u8>(), 0..32),
        any::<u64>(),
        arb_counting_flags(),
        prop::collection::vec(arb_line_entry(), 0..4),
    )
        .prop_map(|(id, bytecode, content_hash, counting_flags, lines)| {
            let def = ContainerDef {
                id,
                bytecode,
                content_hash,
                counting_flags,
                path_hash: 0,
            };
            let lt = ContainerLineTable {
                container_id: id,
                lines,
            };
            (def, lt)
        })
}

fn arb_global_var() -> impl Strategy<Value = GlobalVarDef> {
    (
        arb_def_id(),
        arb_name_id(),
        arb_value_type(),
        arb_value(),
        any::<bool>(),
    )
        .prop_map(
            |(id, name, value_type, default_value, mutable)| GlobalVarDef {
                id,
                name,
                value_type,
                default_value,
                mutable,
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
        prop::collection::vec(".*", 0..8),
    )
        .prop_map(
            |(pairs, variables, list_defs, list_items, externals, name_table)| {
                let (containers, line_tables): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
                StoryData {
                    containers,
                    line_tables,
                    variables,
                    list_defs,
                    list_items,
                    externals,
                    labels: vec![],
                    name_table,
                    list_literals: vec![],
                }
            },
        )
}

// ── Writer invariant tests ──────────────────────────────────────────────────

proptest! {
    /// The writer always produces output whose index satisfies all structural
    /// invariants: valid magic/version, correct file size, monotonically
    /// increasing section offsets within [header_size, file_size].
    #[test]
    fn writer_produces_valid_index(story in arb_story_data()) {
        let mut buf = Vec::new();
        write_inkb(&story, &mut buf);

        // read_inkb_index validates all structural invariants; it must succeed.
        let index = read_inkb_index(&buf).unwrap();

        // file_size matches actual buffer length.
        prop_assert_eq!(index.file_size as usize, buf.len());

        // Correct version.
        prop_assert_eq!(index.version, 1);

        // Exactly 9 sections in canonical order.
        prop_assert_eq!(index.sections.len(), 9);
        prop_assert_eq!(index.sections[0].kind, SectionKind::NameTable);
        prop_assert_eq!(index.sections[1].kind, SectionKind::Variables);
        prop_assert_eq!(index.sections[2].kind, SectionKind::ListDefs);
        prop_assert_eq!(index.sections[3].kind, SectionKind::ListItems);
        prop_assert_eq!(index.sections[4].kind, SectionKind::Externals);
        prop_assert_eq!(index.sections[5].kind, SectionKind::Containers);
        prop_assert_eq!(index.sections[6].kind, SectionKind::LineTables);
        prop_assert_eq!(index.sections[7].kind, SectionKind::Labels);
        prop_assert_eq!(index.sections[8].kind, SectionKind::ListLiterals);

        let header_size = u32::try_from(index.header_size()).unwrap();

        // First section starts at header boundary.
        prop_assert_eq!(index.sections[0].offset, header_size);

        // Offsets are monotonically increasing and within bounds.
        let mut prev = header_size;
        for entry in &index.sections {
            prop_assert!(entry.offset >= prev,
                "section {:?} offset {} < previous {}",
                entry.kind, entry.offset, prev);
            prop_assert!(entry.offset <= index.file_size,
                "section {:?} offset {} > file_size {}",
                entry.kind, entry.offset, index.file_size);
            prev = entry.offset;
        }

        // Section ranges cover the entire post-header region with no gaps.
        let mut covered = index.header_size();
        for entry in &index.sections {
            let range = index.section_range(entry.kind).unwrap();
            prop_assert_eq!(range.start, covered,
                "gap before section {:?}", entry.kind);
            covered = range.end;
        }
        prop_assert_eq!(covered, index.file_size as usize);
    }

    /// Checksum in the header matches the actual CRC-32 of section data.
    #[test]
    fn writer_produces_valid_checksum(story in arb_story_data()) {
        let mut buf = Vec::new();
        write_inkb(&story, &mut buf);

        // Full read validates the checksum; it must succeed.
        let recovered = read_inkb(&buf).unwrap();
        prop_assert_eq!(story, recovered);
    }

    /// Write-then-read is a perfect round-trip for all StoryData values.
    #[test]
    fn write_read_roundtrip(story in arb_story_data()) {
        let mut buf = Vec::new();
        write_inkb(&story, &mut buf);

        let recovered = read_inkb(&buf).unwrap();
        prop_assert_eq!(story, recovered);
    }
}
