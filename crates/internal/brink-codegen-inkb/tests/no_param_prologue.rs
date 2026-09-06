#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `.inkb` v10 (`docs/compiler-spec.md` §"Parameter binding"): a
//! parameterized container carries **no** parameter-binding prologue. Before
//! v10 codegen emitted one `DeclareTemp(slot)` per parameter at offset 0, so
//! arriving at offset 0 bound the parameters however control got there; the
//! VM binds them at entry now, and the container's bytecode starts at its
//! first real statement.
//!
//! The runtime addresses those slots positionally, which is an invariant
//! codegen owes it: parameters must occupy slots `0 … n-1` in declared
//! order. A container that numbers them otherwise is refused at emit time
//! rather than misbinding silently at run time.

use brink_format::{CountingFlags, DefinitionId, DefinitionTag, Opcode};
use brink_ir::lir;

fn id(n: u64) -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, n)
}

fn prov() -> brink_ir::Provenance {
    brink_ir::Provenance::synthetic(brink_ir::NodeClass::Stmt, rowan::TextRange::empty(0.into()))
}

fn param(name: &str, slot: u16, names: &mut Vec<String>) -> lir::Param {
    let name_id = brink_format::NameId(u16::try_from(names.len()).unwrap());
    names.push(name.to_string());
    lir::Param {
        name: name_id,
        slot,
        is_ref: false,
        is_divert: false,
    }
}

/// A function container with `params`, whose body is a single `Nop`-bearing
/// statement we can recognise at offset 0.
fn program_with_params(params: Vec<lir::Param>, names: Vec<String>) -> lir::Program {
    let fn_id = id(2);
    let callee = lir::Container {
        id: fn_id,
        provenance: prov(),
        name: None,
        kind: lir::ContainerKind::Knot,
        params,
        body: vec![lir::Stmt::new(
            lir::StmtKind::Return {
                value: Some(lir::Expr::new(lir::ExprKind::Int(7), prov())),
                is_tunnel: false,
                args: Vec::new(),
            },
            prov(),
        )],
        children: Vec::new(),
        counting_flags: CountingFlags::VISITS,
        temp_slot_count: 4,
        labeled: false,
        inline: false,
        is_function: true,
        local: false,
    };
    lir::Program {
        root: lir::Container {
            id: id(1),
            provenance: prov(),
            name: None,
            kind: lir::ContainerKind::Root,
            params: Vec::new(),
            body: Vec::new(),
            children: vec![callee],
            counting_flags: CountingFlags::empty(),
            temp_slot_count: 0,
            labeled: false,
            inline: false,
            is_function: false,
            local: false,
        },
        globals: Vec::new(),
        lists: Vec::new(),
        list_items: Vec::new(),
        externals: Vec::new(),
        name_table: names,
        struct_shapes: Vec::new(),
        private_defs: Vec::new(),
        aliases: Vec::new(),
        file_paths: std::collections::BTreeMap::new(),
    }
}

fn first_opcode(story: &brink_format::StoryData, container: DefinitionId) -> Opcode {
    let c = story
        .containers
        .iter()
        .find(|c| c.id == container)
        .expect("container emitted");
    let mut offset = 0;
    Opcode::decode(&c.bytecode, &mut offset).expect("decodes")
}

#[test]
fn a_parameterized_container_emits_no_binding_prologue() {
    let mut names = Vec::new();
    let params = vec![
        param("a", 0, &mut names),
        param("b", 1, &mut names),
        param("c", 2, &mut names),
    ];
    let story = brink_codegen_inkb::emit(&program_with_params(params, names)).unwrap();

    let callee = story.containers.iter().find(|c| c.id == id(2)).unwrap();
    assert_eq!(
        callee.param_count, 3,
        "the arity the VM binds from is still recorded"
    );
    assert!(
        !matches!(first_opcode(&story, id(2)), Opcode::DeclareTemp(_)),
        "offset 0 must be the body's first instruction, not a parameter prologue: {:?}",
        first_opcode(&story, id(2))
    );

    // Nothing anywhere in the container binds a parameter slot: the whole
    // prologue is gone, not merely moved off offset 0.
    let mut offset = 0;
    while offset < callee.bytecode.len() {
        let op = Opcode::decode(&callee.bytecode, &mut offset).unwrap();
        assert!(
            !matches!(op, Opcode::DeclareTemp(slot) if slot < 3),
            "found a parameter-binding {op:?} in a v10 container"
        );
    }
}

#[test]
fn a_container_without_parameters_is_unchanged() {
    let story = brink_codegen_inkb::emit(&program_with_params(Vec::new(), Vec::new())).unwrap();
    let callee = story.containers.iter().find(|c| c.id == id(2)).unwrap();
    assert_eq!(callee.param_count, 0);
    assert!(!callee.bytecode.is_empty());
}

#[test]
fn parameters_numbered_out_of_declared_order_are_refused() {
    // The runtime binds slot `i` from the `i`-th argument, so a container
    // whose second parameter lives in slot 5 would misbind every call.
    let mut names = Vec::new();
    let params = vec![param("a", 0, &mut names), param("b", 5, &mut names)];
    let err = brink_codegen_inkb::emit(&program_with_params(params, names))
        .expect_err("a container that numbers its parameters oddly must not emit");
    let msg = err.to_string();
    assert!(
        msg.contains("temp slot 5") && msg.contains("not 1"),
        "the error should name the offending slot and the one required: {msg}"
    );
}
