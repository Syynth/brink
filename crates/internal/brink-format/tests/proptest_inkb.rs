#![allow(clippy::unwrap_used)]

use brink_format::{
    AddressPath, ClosureEnvEntry, ContainerDef, CountingFlags, DefinitionId, DefinitionTag,
    ExternalFnDef, GlobalVarDef, LineContent, LineEntry, LinePart, ListDef, ListItemDef, ListValue,
    MapKey, NameId, OrderedMap, PluralCategory, ScopeLineTable, SectionKind, SelectKey, ShapeId,
    SlotInfo, SourceLocation, StoryData, Value, ValueType, read_inkb, read_inkb_index, write_inkb,
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
        ".*".prop_map(SelectKey::Keyword),
    ]
}

fn arb_line_part_leaf() -> impl Strategy<Value = LinePart> {
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

/// `LinePart::Span` (#1716, `docs/prose-dialect-spec.md` §4.4) — nested,
/// like `arb_value`'s collection variants above. Bounded via `prop_recursive`
/// (depth 3, up to 16 total nodes, width 3 per span) so generated cases stay
/// small and shrinkable, and so it exercises `escape_string`/`unescape_string`
/// over span names and attr values through the same `".*"` domain the other
/// leaves already use (arbitrary Unicode, including the escape set's own
/// `\< \{ \# \\` characters).
fn arb_line_part() -> impl Strategy<Value = LinePart> {
    arb_line_part_leaf().prop_recursive(3, 16, 3, |inner| {
        (
            ".*",
            prop::collection::vec((".*", ".*"), 0..3),
            prop::collection::vec(inner, 0..3),
        )
            .prop_map(|(name, attrs, children)| LinePart::Span {
                name,
                attrs,
                children,
            })
    })
}

fn arb_line_content() -> impl Strategy<Value = LineContent> {
    prop_oneof![
        ".*".prop_map(LineContent::Plain),
        prop::collection::vec(arb_line_part(), 1..4).prop_map(LineContent::Template),
    ]
}

fn arb_slot_info() -> impl Strategy<Value = SlotInfo> {
    (any::<u8>(), ".*").prop_map(|(index, name)| SlotInfo { index, name })
}

fn arb_source_location() -> impl Strategy<Value = SourceLocation> {
    (".*", any::<u32>(), any::<u32>()).prop_map(|(file, range_start, range_end)| SourceLocation {
        file,
        range_start,
        range_end,
    })
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

fn arb_value_type() -> impl Strategy<Value = ValueType> {
    prop_oneof![
        Just(ValueType::Int),
        Just(ValueType::Float),
        Just(ValueType::Bool),
        Just(ValueType::String),
        Just(ValueType::DivertTarget),
        Just(ValueType::Null),
        Just(ValueType::FnRef),
        Just(ValueType::Closure),
        // Collection/record value types (value-model-spec §4; issue #672
        // workstream B item 2 — these were missing from the declared-type
        // side of this fuzzer even though `arb_value` below can now produce
        // `Array`/`Map`/`Record` payloads).
        Just(ValueType::Array),
        Just(ValueType::Map),
        Just(ValueType::Record),
        // T1d handle value type (docs/t1d-spec.md §2): `arb_value` below can
        // now produce a `Handle` payload, same rationale as the collection
        // types above.
        Just(ValueType::Handle),
        // T1e projection value type (docs/t1e-spec.md §3): `arb_value` below
        // can now produce a `Projection` payload, same rationale.
        Just(ValueType::Projection),
        // The ink LIST value type (value-model-spec §4's nominal list
        // domain, `VAL_LIST` on the wire): issue #746 — `arb_value` below
        // can now produce a `List` payload, same rationale as every other
        // family above.
        Just(ValueType::List),
        // NS-A8 tower value types (docs/tower-mini-spec.md T5): `arb_value`
        // below can now produce tower payloads, same rationale.
        Just(ValueType::Vec2),
        Just(ValueType::Vec3),
        Just(ValueType::Vec4),
        Just(ValueType::Quat),
        Just(ValueType::Mat2),
        Just(ValueType::Mat3),
        Just(ValueType::Mat4),
        // NS-A7 weighted table value type (docs/stdlib-spec.md §8), same
        // rationale.
        Just(ValueType::Weighted),
    ]
}

/// A single `ProjSegment` — scalar-only payloads (never a full recursive
/// `Value`) so `arb_value_leaf` can stay a leaf generator (T1e, segment
/// values are always scalar in practice: an index, a map key, or a struct
/// field name).
fn arb_proj_segment() -> impl Strategy<Value = brink_format::ProjSegment> {
    prop_oneof![
        any::<i32>().prop_map(brink_format::ProjSegment::Index),
        any::<i32>().prop_map(|n| brink_format::ProjSegment::Key(Value::Int(n))),
        ".*".prop_map(|s: String| brink_format::ProjSegment::Key(Value::String(s.into()))),
        any::<bool>().prop_map(|b| brink_format::ProjSegment::Key(Value::Bool(b))),
    ]
}

fn arb_map_key() -> impl Strategy<Value = MapKey> {
    prop_oneof![
        any::<i32>().prop_map(MapKey::Int),
        ".*".prop_map(|s: String| MapKey::Str(s.into())),
        any::<bool>().prop_map(MapKey::Bool),
    ]
}

/// Leaf values (never recurse). `arb_value` below extends this with
/// `Array`/`Map`/`Record` — the value-model-spec §4 collection types — so
/// `write_read_roundtrip` actually exercises the encode/decode path for
/// `GlobalVarDef::default_value` holding a nested collection (issue #672
/// workstream B item 2, "value -> inkb bytes -> value == identity"). Bounded
/// via `prop_recursive` (depth 3, up to 16 total nodes, width 4 per
/// collection) so generated cases stay small and shrinkable.
fn arb_value_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<i32>().prop_map(Value::Int),
        any::<f32>().prop_map(Value::Float),
        any::<bool>().prop_map(Value::Bool),
        ".*".prop_map(|s: String| Value::String(s.into())),
        arb_def_id().prop_map(Value::DivertTarget),
        Just(Value::Null),
        // T1c function values (#700): a `#fn(…)` baked into a declaration
        // default reaches the wire as a global's `default_value`.
        arb_def_id().prop_map(Value::FnRef),
        arb_closure(),
        // NS-A5 range values (F7, `VAL_RANGE` on the wire): a flat scalar
        // leaf — start/end/inclusive round-trip bit-for-bit (the written
        // form is preserved; only *equality* is content-based).
        (any::<i32>(), any::<i32>(), any::<bool>())
            .prop_map(|(start, end, inclusive)| Value::range(start, end, inclusive)),
        // T1d handle values (docs/t1d-spec.md §2): the reserved `VAL_HANDLE`
        // tag's first emission — round-tripped here like every other value
        // tag.
        (arb_name_id(), any::<u64>()).prop_map(|(kind, id)| Value::handle(kind, id)),
        // T1e projection values (docs/t1e-spec.md §3): the reserved
        // `VAL_PROJECTION` tag's first emission — round-tripped here like
        // every other value tag. Segment kind `2=range` stays RESERVED
        // (never generated — `arb_proj_segment` has no such arm).
        (
            arb_def_id(),
            prop::collection::vec(arb_proj_segment(), 0..4)
        )
            .prop_map(|(cell, segments)| Value::projection(cell, segments)),
        // Ink LIST values (value-model-spec §4, `VAL_LIST` on the wire):
        // issue #746 — the one member of this leaf family that was still
        // missing from this fuzzer even though the `.inkb` writer/reader
        // have supported `VAL_LIST` since the format's original list
        // support landed. `items`/`origins` are both `ListItem`-tagged
        // `DefinitionId`s in production, but the round-trip codec never
        // inspects the tag, so any `arb_def_id()` exercises the same
        // encode/decode arms.
        (
            prop::collection::vec(arb_def_id(), 0..4),
            prop::collection::vec(arb_def_id(), 0..4),
        )
            .prop_map(|(items, origins)| Value::List(ListValue { items, origins }.into())),
        // NS-A8 tower values (docs/tower-mini-spec.md T5): the seven lane
        // families, lanes drawn from proptest's default finite-f32 domain
        // and rebuilt through glam's explicit array constructors — the
        // round-trip pins the hand-serialized little-endian lane wire.
        any::<[f32; 2]>().prop_map(|l| Value::Vec2(glam::Vec2::from_array(l))),
        any::<[f32; 3]>().prop_map(|l| Value::Vec3(glam::Vec3::from_array(l))),
        any::<[f32; 4]>().prop_map(|l| Value::Vec4(glam::Vec4::from_array(l))),
        any::<[f32; 4]>().prop_map(|l| Value::Quat(glam::Quat::from_array(l))),
        any::<[f32; 4]>().prop_map(|l| Value::Mat2(glam::Mat2::from_cols_array(&l))),
        any::<[f32; 9]>().prop_map(|l| Value::Mat3(glam::Mat3::from_cols_array(&l))),
        any::<[f32; 16]>().prop_map(|l| Value::Mat4(glam::Mat4::from_cols_array(&l))),
    ]
}

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
            (any::<u32>(), prop::collection::vec(inner.clone(), 0..4))
                .prop_map(|(shape, fields)| Value::record(ShapeId(shape), fields)),
            // Weighted tables (NS-A7, docs/stdlib-spec.md §8): 1-3
            // positive-weight entries (the evidence-by-construction
            // invariant the reader enforces), values from the full
            // recursive universe.
            prop::collection::vec((1i32..=i32::MAX, inner.clone()), 1..4).prop_map(Value::weighted),
            // Option values (NS-A1, docs/stdlib-spec.md §1.4): both
            // variants, payload from the full recursive universe so
            // `some(none)`/`some(#[..])` nesting rides the writer/reader
            // fuzz too.
            prop::option::of(inner).prop_map(|payload| match payload {
                None => Value::none(),
                Some(v) => Value::some(v),
            }),
        ]
    })
}

/// A `Value::Closure` with a small bound-env of `val` (`Int` snapshot) and
/// `ref` (`VariablePointer`) entries — the two payload shapes T1c produces.
fn arb_closure() -> impl Strategy<Value = Value> {
    let entry = (arb_name_id(), any::<bool>(), arb_def_id(), any::<i32>()).prop_map(
        |(name, is_ref, cell, n)| ClosureEnvEntry {
            name,
            is_ref,
            payload: if is_ref {
                Value::VariablePointer(cell)
            } else {
                Value::Int(n)
            },
        },
    );
    (arb_def_id(), prop::collection::vec(entry, 0..4))
        .prop_map(|(target, env)| Value::closure(target, env))
}

/// Structural exhaustiveness guard (issue #667, mirroring the identical guard
/// `proptest_inkt.rs` added for #883/#397): a match over every current
/// [`Value`] variant with **no wildcard arm**, so this fails to compile the
/// moment a new variant is added to the enum. Never called — the only
/// purpose is the compile-time forcing function: whoever adds a `Value`
/// variant must also add an arm here (and, per the doc, teach
/// `arb_value`/`arb_value_leaf` above to generate it), instead of the new
/// variant silently escaping this `.inkb` writer/reader fuzz coverage the way
/// `Record`'s `value_to_js` wildcard let it escape the wasm marshal boundary
/// (#667: PR #664's review finding).
#[expect(dead_code, reason = "compile-time-only exhaustiveness guard, see doc")]
fn assert_value_variants_exhaustive(value: &Value) {
    match value {
        Value::Int(_)
        | Value::Float(_)
        | Value::Bool(_)
        | Value::String(_)
        | Value::List(_)
        | Value::DivertTarget(_)
        | Value::VariablePointer(_)
        | Value::TempPointer { .. }
        | Value::Null
        | Value::FragmentRef(_)
        | Value::Array(_)
        | Value::Map(_)
        | Value::Record { .. }
        | Value::FnRef(_)
        | Value::Closure(_)
        | Value::Handle { .. }
        | Value::Projection(_)
        | Value::OptionVal(_)
        | Value::Range { .. }
        | Value::Vec2(_)
        | Value::Vec3(_)
        | Value::Vec4(_)
        | Value::Quat(_)
        | Value::Mat2(_)
        | Value::Mat3(_)
        | Value::Mat4(_)
        | Value::Weighted(_) => {}
    }
}

fn arb_container_with_lines() -> impl Strategy<Value = (ContainerDef, ScopeLineTable)> {
    (
        arb_def_id(),
        prop::collection::vec(any::<u8>(), 0..32),
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

fn arb_global_var() -> impl Strategy<Value = GlobalVarDef> {
    (
        arb_def_id(),
        arb_name_id(),
        arb_value_type(),
        arb_value(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(id, name, value_type, default_value, mutable, local)| GlobalVarDef {
                id,
                name,
                value_type,
                default_value,
                mutable,
                local,
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
        prop::collection::vec(".*", 0..8),
    )
        .prop_map(
            |(pairs, variables, list_defs, list_items, externals, address_paths, name_table)| {
                let (containers, line_tables): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
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
                    effect_rows: vec![],
                    frame_shapes: Vec::new(),
                    debug_info: None,
                    source_checksum: 0,
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
        prop_assert_eq!(index.version, 6);

        // Exactly 14 sections in canonical order.
        prop_assert_eq!(index.sections.len(), 14);
        prop_assert_eq!(index.sections[0].kind, SectionKind::NameTable);
        prop_assert_eq!(index.sections[1].kind, SectionKind::Variables);
        prop_assert_eq!(index.sections[2].kind, SectionKind::ListDefs);
        prop_assert_eq!(index.sections[3].kind, SectionKind::ListItems);
        prop_assert_eq!(index.sections[4].kind, SectionKind::Externals);
        prop_assert_eq!(index.sections[5].kind, SectionKind::Containers);
        prop_assert_eq!(index.sections[6].kind, SectionKind::LineTables);
        prop_assert_eq!(index.sections[7].kind, SectionKind::Labels);
        prop_assert_eq!(index.sections[8].kind, SectionKind::ListLiterals);
        prop_assert_eq!(index.sections[9].kind, SectionKind::AddressPaths);
        prop_assert_eq!(index.sections[10].kind, SectionKind::LiteralPool);
        prop_assert_eq!(index.sections[11].kind, SectionKind::StructShapes);
        prop_assert_eq!(index.sections[12].kind, SectionKind::EffectRows);
        prop_assert_eq!(index.sections[13].kind, SectionKind::AliasTable);

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
        let mut recovered = read_inkb(&buf).unwrap();
        // source_checksum is set from the binary header, not semantic data.
        recovered.source_checksum = story.source_checksum;
        prop_assert_eq!(story, recovered);
    }

    /// Write-then-read is a perfect round-trip for all StoryData values.
    #[test]
    fn write_read_roundtrip(story in arb_story_data()) {
        let mut buf = Vec::new();
        write_inkb(&story, &mut buf);

        let mut recovered = read_inkb(&buf).unwrap();
        recovered.source_checksum = story.source_checksum;
        prop_assert_eq!(story, recovered);
    }
}
