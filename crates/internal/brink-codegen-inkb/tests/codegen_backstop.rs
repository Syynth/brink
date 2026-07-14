#![allow(clippy::unwrap_used, clippy::panic)]

//! #586: codegen defense-in-depth backstop for out-of-loop
//! `LogicBreak`/`LogicContinue`.
//!
//! `brink-ir::lir::lower` rejects `break`/`continue` outside any
//! `while`/`for` loop at LIR-lowering time (E057, non-suppressible — see
//! `crates/internal/brink-ir/tests/lir_lowering.rs`'s `#577` tests), so a
//! `Program` built by the real compiler pipeline never contains an
//! unguarded `LogicBreak`/`LogicContinue`. That invariant is enforced by a
//! *different* crate, though — `brink-codegen-inkb` has no independent way
//! to verify it, and until this fix, `container.rs`'s `emit_stmt` trusted
//! it unconditionally: an out-of-loop `LogicBreak`/`LogicContinue` would
//! fall through to `Opcode::Jump(0)` with no patch site, corrupting the
//! bytecode silently instead of failing.
//!
//! These tests hand-assemble a minimal `lir::Program` (bypassing
//! `brink-ir::lir::lower` entirely, the only way to construct the
//! otherwise-unreachable input) to prove `emit()` now refuses it with a
//! real `Err(CodegenError)` instead of emitting the dangling jump.

use brink_format::{CountingFlags, DefinitionId, DefinitionTag};
use brink_ir::lir;

fn root_id() -> DefinitionId {
    DefinitionId::new(DefinitionTag::Address, 1)
}

/// A minimal, otherwise-empty `Program` whose root container body is
/// `body` — enough surface for `emit()` to walk without hitting any other
/// (irrelevant) codegen path.
fn program_with_root_body(body: Vec<lir::Stmt>) -> lir::Program {
    lir::Program {
        root: lir::Container {
            id: root_id(),
            name: None,
            kind: lir::ContainerKind::Root,
            params: Vec::new(),
            body,
            children: Vec::new(),
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
    }
}

#[test]
fn well_formed_program_still_emits_successfully() {
    // Control case: a `Program` with no loop-control statements at all
    // compiles fine — proves the backstop doesn't false-positive on
    // ordinary input.
    let program = program_with_root_body(vec![lir::Stmt::EndOfLine]);
    let story = brink_codegen_inkb::emit(&program);
    assert!(story.is_ok(), "expected Ok, got {story:?}");
}

#[test]
fn break_in_a_well_formed_loop_still_emits_successfully() {
    // Control case: a `break` that *does* have an enclosing `LogicWhile`
    // still compiles — proves the backstop only fires on a genuinely empty
    // `loop_stack`, not on every `LogicBreak`.
    let program = program_with_root_body(vec![lir::Stmt::LogicWhile(lir::LogicWhile {
        condition: lir::Expr::Bool(true),
        body: vec![lir::Stmt::LogicBreak],
        post: Vec::new(),
    })]);
    let story = brink_codegen_inkb::emit(&program);
    assert!(story.is_ok(), "expected Ok, got {story:?}");
}

#[test]
fn out_of_loop_logic_break_is_a_hard_codegen_error() {
    let program = program_with_root_body(vec![lir::Stmt::LogicBreak]);
    let err = brink_codegen_inkb::emit(&program)
        .expect_err("an out-of-loop LogicBreak must not silently compile");
    let message = err.to_string();
    assert!(
        message.contains("break") && message.contains("586"),
        "error message should name the construct and reference #586: {message}"
    );
}

#[test]
fn out_of_loop_logic_continue_is_a_hard_codegen_error() {
    let program = program_with_root_body(vec![lir::Stmt::LogicContinue]);
    let err = brink_codegen_inkb::emit(&program)
        .expect_err("an out-of-loop LogicContinue must not silently compile");
    let message = err.to_string();
    assert!(
        message.contains("continue") && message.contains("586"),
        "error message should name the construct and reference #586: {message}"
    );
}

#[test]
fn out_of_loop_break_after_a_sibling_loop_still_errors() {
    // A `break` textually *after* a well-formed loop (sibling, not
    // nested) — proves the emitter's `loop_stack` is correctly empty again
    // once the loop's own emission finishes, not leaking across siblings.
    let program = program_with_root_body(vec![
        lir::Stmt::LogicWhile(lir::LogicWhile {
            condition: lir::Expr::Bool(true),
            body: vec![lir::Stmt::EndOfLine],
            post: Vec::new(),
        }),
        lir::Stmt::LogicBreak,
    ]);
    let err = brink_codegen_inkb::emit(&program)
        .expect_err("a break after (not inside) a loop must still error");
    assert!(err.to_string().contains("break"));
}
