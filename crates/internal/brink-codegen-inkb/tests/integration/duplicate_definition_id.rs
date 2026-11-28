#![allow(clippy::unwrap_used, clippy::panic)]

//! #1673: codegen-boundary guard against two containers sharing a
//! `DefinitionId`.
//!
//! `brink-ir::lir::lower` is expected to mint a distinct `DefinitionId` for
//! every container it emits, but #1504 proved that invariant can break in
//! practice: unqualified anonymous scope paths (`g-0`, `c0`, …) let two
//! different files' root-level weave content collide on the same id. When
//! that happened, nothing in codegen noticed — the collision reached the
//! runtime silently. The linker's address map is last-write-wins, so the
//! second container to compile quietly overwrote the first's entry in
//! `state.chunks`/`state.addresses`, and a player picking a choice from one
//! container ran the *other* container's body (wrong player-visible
//! output).
//!
//! These tests hand-assemble a minimal `lir::Program` (bypassing
//! `brink-ir::lir::lower`, the only way to construct the otherwise
//! structurally-guaranteed-unreachable input — same technique as
//! `codegen_backstop.rs`'s #586 tests) reproducing the #1504 shape: two
//! sibling root-level containers assigned the same `DefinitionId`, standing
//! in for two files' root weave content colliding. `emit()` must refuse it
//! with a real `Err(CodegenError)` instead of silently letting the second
//! container clobber the first.

use brink_format::{CountingFlags, DefinitionId, DefinitionTag};
use brink_ir::lir;

fn root_id() -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, 1)
}

/// A placeholder provenance (issue #3183) — this fixture has no real
/// source text behind it.
fn test_provenance() -> brink_ir::Provenance {
    brink_ir::Provenance::synthetic(brink_ir::NodeClass::Stmt, rowan::TextRange::empty(0.into()))
}

/// A container with no body/children of its own, suitable as a leaf in the
/// hand-assembled tree.
fn leaf_container(id: DefinitionId, kind: lir::ContainerKind) -> lir::Container {
    lir::Container {
        id,
        provenance: test_provenance(),
        name: None,
        kind,
        params: Vec::new(),
        body: vec![lir::Stmt::new(lir::StmtKind::EndOfLine, test_provenance())],
        children: Vec::new(),
        counting_flags: CountingFlags::empty(),
        temp_slot_count: 0,
        labeled: false,
        inline: false,
        is_function: false,
        local: false,
    }
}

/// A minimal, otherwise-empty `Program` whose root has the given children —
/// enough surface for `emit()` to walk without hitting any other
/// (irrelevant) codegen path.
fn program_with_root_children(children: Vec<lir::Container>) -> lir::Program {
    lir::Program {
        root: lir::Container {
            id: root_id(),
            provenance: test_provenance(),
            name: None,
            kind: lir::ContainerKind::Root,
            params: Vec::new(),
            body: Vec::new(),
            children,
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
        name_table: Vec::new(),
        struct_shapes: Vec::new(),
        private_defs: Vec::new(),
        aliases: Vec::new(),
        file_paths: std::collections::BTreeMap::new(),
    }
}

#[test]
fn distinct_ids_still_emit_successfully() {
    // Control case: two sibling containers with distinct ids compile fine —
    // proves the guard doesn't false-positive on ordinary input.
    let a = leaf_container(
        DefinitionId::new(DefinitionTag::Address, 100),
        lir::ContainerKind::Gather,
    );
    let b = leaf_container(
        DefinitionId::new(DefinitionTag::Address, 200),
        lir::ContainerKind::Gather,
    );
    let program = program_with_root_children(vec![a, b]);
    let story = brink_codegen_inkb::emit(&program);
    assert!(story.is_ok(), "expected Ok, got {story:?}");
}

#[test]
fn two_containers_sharing_a_definition_id_is_a_hard_codegen_error() {
    // The #1504 shape: two sibling containers (standing in for two files'
    // root-level weave content) assigned the *same* DefinitionId by an
    // upstream id-derivation defect.
    let shared_id = DefinitionId::new(DefinitionTag::Address, 42);
    let a = leaf_container(shared_id, lir::ContainerKind::Gather);
    let b = leaf_container(shared_id, lir::ContainerKind::Gather);
    let program = program_with_root_children(vec![a, b]);

    let err = brink_codegen_inkb::emit(&program)
        .expect_err("two containers sharing a DefinitionId must not silently compile");
    let message = err.to_string();
    assert!(
        message.contains("duplicate DefinitionId") && message.contains("1673"),
        "error message should name the defect and reference #1673: {message}"
    );
}

#[test]
fn duplicate_deeper_in_the_tree_is_still_caught() {
    // The colliding pair need not be direct siblings of root — reproduces
    // the #1504 shape more faithfully, where the collision is between two
    // different files' root scopes, each with its own nested content.
    let shared_id = DefinitionId::new(DefinitionTag::Address, 7);
    let nested_a = lir::Container {
        children: vec![leaf_container(shared_id, lir::ContainerKind::Gather)],
        ..leaf_container(
            DefinitionId::new(DefinitionTag::Address, 8),
            lir::ContainerKind::Stitch,
        )
    };
    let nested_b = lir::Container {
        children: vec![leaf_container(shared_id, lir::ContainerKind::Gather)],
        ..leaf_container(
            DefinitionId::new(DefinitionTag::Address, 9),
            lir::ContainerKind::Stitch,
        )
    };
    let program = program_with_root_children(vec![nested_a, nested_b]);

    let err = brink_codegen_inkb::emit(&program)
        .expect_err("a duplicate DefinitionId nested in the tree must still be caught");
    assert!(err.to_string().contains("duplicate DefinitionId"));
}
