//! Per-family write→read round-trip tests for the four `.inkt` opcode/value
//! families that were write-only until issue #871: record ops, conversion
//! intrinsics, fn-value ops (`VAL_FN_REF`/`VAL_CLOSURE`), and projection ops
//! (`VAL_PROJECTION`). Each test builds a `StoryData` containing the family's
//! opcodes (in container bytecode) and, where applicable, its literal value
//! form (as a global default), writes it to `.inkt` text, reads it back, and
//! asserts byte-for-byte structural equality — the exact test that should
//! have caught the original write/read asymmetry (the #742 lesson).

#![cfg(feature = "inkt")]
#![allow(clippy::unwrap_used)]

use brink_format::{
    ContainerDef, CountingFlags, DefinitionId, DefinitionTag, GlobalVarDef, NameId, Opcode,
    ProjSegment, ScopeLineTable, ShapeId, StoryData, Value,
};

fn empty_story() -> StoryData {
    StoryData {
        containers: vec![],
        line_tables: vec![],
        variables: vec![],
        list_defs: vec![],
        list_items: vec![],
        externals: vec![],
        addresses: vec![],
        address_paths: vec![],
        name_table: vec![],
        list_literals: vec![],
        literal_pool: vec![],
        struct_shapes: vec![],
        private_defs: vec![],
        alias_table: vec![],
        source_checksum: 0,
    }
}

/// Build a scope-owning container with the given bytecode and push it (plus
/// the empty `ScopeLineTable` `read_inkt` always synthesizes for a
/// scope-owning container, lines or not) onto `story`.
fn push_container_with_bytecode(story: &mut StoryData, hash: u64, ops: &[Opcode]) {
    let id = DefinitionId::new(DefinitionTag::Address, hash);
    let mut bytecode = Vec::new();
    for op in ops {
        op.encode(&mut bytecode);
    }
    story.containers.push(ContainerDef {
        id,
        scope_id: id,
        name: None,
        bytecode,
        counting_flags: CountingFlags::empty(),
        path_hash: 0,
        param_count: 0,
        params: Vec::new(),
        local: false,
    });
    story.line_tables.push(ScopeLineTable {
        scope_id: id,
        lines: Vec::new(),
    });
}

fn global(hash: u64, default_value: Value) -> GlobalVarDef {
    GlobalVarDef {
        id: DefinitionId::new(DefinitionTag::GlobalVar, hash),
        name: NameId(0),
        value_type: default_value.value_type(),
        default_value,
        mutable: false,
        local: false,
    }
}

/// Assert that writing then reading `story` reproduces it exactly, and that
/// the decoded bytecode of every container matches the original opcodes.
fn assert_roundtrips(story: &StoryData) {
    let mut buf = String::new();
    brink_format::write_inkt(story, &mut buf).unwrap();
    let recovered = brink_format::read_inkt(&buf).unwrap();
    assert_eq!(*story, recovered, "inkt text:\n{buf}");
}

#[test]
fn record_family_roundtrips() {
    let mut story = empty_story();
    push_container_with_bytecode(
        &mut story,
        1,
        &[
            Opcode::RecordNew(7),
            Opcode::RecordGetDyn(3),
            Opcode::RecordSetDyn(4),
            Opcode::RecordGet(0),
            Opcode::RecordSet(1),
        ],
    );
    story.variables.push(global(
        1,
        Value::record(
            ShapeId(7),
            vec![Value::Int(1), Value::String("hp".into()), Value::Bool(true)],
        ),
    ));
    assert_roundtrips(&story);
}

#[test]
fn conversion_intrinsics_family_roundtrips() {
    let mut story = empty_story();
    push_container_with_bytecode(
        &mut story,
        2,
        &[
            Opcode::ConvertInt,
            Opcode::ConvertFloat,
            Opcode::ConvertString,
        ],
    );
    assert_roundtrips(&story);
}

#[test]
fn fn_value_family_roundtrips() {
    let mut story = empty_story();
    let target = DefinitionId::new(DefinitionTag::Address, 42);
    push_container_with_bytecode(
        &mut story,
        3,
        &[
            Opcode::PushFnRef(target),
            Opcode::MakeClosure {
                target,
                bound_count: 2,
            },
            Opcode::CallValue(3),
            Opcode::BindValue(1),
        ],
    );
    story.variables.push(global(2, Value::FnRef(target)));
    story.variables.push(global(
        3,
        Value::closure(
            target,
            vec![
                brink_format::ClosureEnvEntry {
                    name: NameId(5),
                    is_ref: false,
                    payload: Value::Int(9),
                },
                brink_format::ClosureEnvEntry {
                    name: NameId(6),
                    is_ref: true,
                    payload: Value::VariablePointer(target),
                },
            ],
        ),
    ));
    assert_roundtrips(&story);
}

#[test]
fn projection_family_roundtrips() {
    let mut story = empty_story();
    let root = DefinitionId::new(DefinitionTag::GlobalVar, 99);
    push_container_with_bytecode(
        &mut story,
        4,
        &[
            Opcode::MakeProjection {
                root,
                segment_count: 2,
            },
            Opcode::ProjRead,
            Opcode::ProjWrite,
        ],
    );
    story.variables.push(global(
        4,
        Value::projection(
            root,
            vec![
                ProjSegment::Index(3),
                ProjSegment::Key(Value::String("field".into())),
            ],
        ),
    ));
    assert_roundtrips(&story);
}

/// All four families together in one story, with the full opcode+value
/// surface exercised at once — closes the gap as a combined regression, not
/// just per-family in isolation.
#[test]
fn all_four_families_together_roundtrip() {
    let mut story = empty_story();
    let target = DefinitionId::new(DefinitionTag::Address, 42);
    let root = DefinitionId::new(DefinitionTag::GlobalVar, 99);
    push_container_with_bytecode(
        &mut story,
        5,
        &[
            Opcode::RecordNew(1),
            Opcode::ConvertInt,
            Opcode::PushFnRef(target),
            Opcode::MakeProjection {
                root,
                segment_count: 1,
            },
            Opcode::ProjRead,
        ],
    );
    story
        .variables
        .push(global(5, Value::record(ShapeId(1), vec![Value::Int(1)])));
    story.variables.push(global(6, Value::FnRef(target)));
    story.variables.push(global(
        7,
        Value::projection(root, vec![ProjSegment::Index(0)]),
    ));
    assert_roundtrips(&story);
}
