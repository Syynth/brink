//! Issue #1727: a lambda-lifted function's `DefinitionId` is minted **once**,
//! at HIR time (`hir::stamp_container_ids`), instead of being independently
//! re-derived by LIR lowering from its own live `ctx.scope_path`.
//!
//! RULED 2026-08-02 (`docs/decision-log.md`): the rejected shape was "a v1
//! scope covering only non-nested lambdas" — the whole point of the ruling
//! is a lambda declared **inside a weave-level `Conditional`/`Sequence`/
//! `ChoiceSet` branch**, because `ctx.scope_path` mutates while descending
//! into one (`brink-ir/src/lir/lower/mod.rs`), and the pre-#1727 HIR-time
//! stamping pass (`hir::stamp_container_ids`) never walked *expressions* at
//! all — it only tracked that same scope nesting for the branch/gather/
//! choice **container** ids it already stamped. These tests exercise
//! exactly that nested shape, not a lambda sitting directly in a knot body
//! (the shape #1680's own characterization test already covers).
//!
//! Regression-test discipline (house rule 20a): reverting just the new
//! per-expression scanning `hir::stamp` added — the `stamp_lambdas_in_expr`/
//! `stamp_lambdas_in_block_stmts`/`stamp_lambdas_in_content`/... family and
//! their call sites inside `stamp_stmt` — while leaving
//! `LambdaExpr::container_id` and `lower_lambda`'s read of it untouched
//! makes [`nested_lambda_gets_a_hir_minted_container_id`] fail: the branch's
//! `~ { let f = … }` `TempDecl` initializer would never be visited, so the
//! lambda's `container_id` stays `None`. Verified by hand before writing
//! this file (temporarily reverting the `hir::Stmt::LogicBlock` arm back to
//! its old empty no-op): the test failed with "expected a lambda id to be
//! stamped inside a weave conditional branch, got None", confirming it
//! actually exercises the new code, not a vacuously-true assertion.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map for these single-file fixtures — \
              brink_ir::determinism::LookupMap is pub(crate) and invisible \
              to this external test-binary crate, same allow \
              lir_lowering.rs's own file doc carries for the identical reason"
)]

use std::collections::HashMap;

use brink_ir::hir::lower_native;
use brink_ir::{BlockStmt, Expr, FileId, HirFile, LambdaExpr, Stmt};

/// A `flow`'s weave-level `{if …}` conditional — as opposed to a `fn`'s
/// code-ground `if`, which lowers to a T1b `BlockStmt::If` and never
/// mutates `ctx.scope_path` at all (established fact this module's doc
/// leans on: a T1b block never produces a LIR container, so nothing inside
/// one needs scope-path nesting) — with a lambda declared inside branch
/// 0's own `~ { … }` logic block.
const SOURCE: &str = "\
flow bumped() {
  {if true {
    ~ { let f = |x| x + 1; }
    Yes.
  } else {
    No.
  }}
}
";

/// Parse, lower, analyze, and stamp `SOURCE` — mirroring
/// `brink_ir::lir::build_prelude`'s own step 0 (normalize + stamp) exactly,
/// the same sequence every real compile runs.
fn lower_and_stamp(source: &str) -> HirFile {
    let parsed = brink_syntax_native::parse(source);
    assert!(
        parsed.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let file_id = FileId(0);
    let (mut hir, manifest, diags) = lower_native::lower(file_id, &parsed.tree());
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );
    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &brink_ir::SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let file_paths: HashMap<FileId, String> = HashMap::new();
    let mut slice = [(file_id, hir)];
    brink_ir::stamp_container_ids(&mut slice, &result.index, &file_paths);
    let [(_, stamped)] = slice;
    stamped
}

/// Navigate straight to the lambda `SOURCE` declares inside the `if`
/// branch's logic block — the fixture's shape is fixed, so a direct match
/// is clearer than a generic visitor for a test this targeted.
fn nested_lambda(hir: &HirFile) -> &LambdaExpr {
    let Stmt::Conditional(cond) = &hir.knots[0].body.stmts[0] else {
        panic!(
            "expected the flow's first statement to be a weave Conditional, got {:?}",
            hir.knots[0].body.stmts[0]
        );
    };
    let branch0 = cond
        .branches
        .first()
        .expect("the `if` conditional has an `if` branch");
    let Stmt::LogicBlock(lb) = &branch0.body.stmts[0] else {
        panic!(
            "expected branch 0's first statement to be a LogicBlock, got {:?}",
            branch0.body.stmts[0]
        );
    };
    let BlockStmt::TempDecl(t) = &lb.stmts[0] else {
        panic!(
            "expected the logic block's first statement to be a TempDecl, got {:?}",
            lb.stmts[0]
        );
    };
    let Some(Expr::Lambda(l)) = t.value.as_ref() else {
        panic!(
            "expected the TempDecl's initializer to be a lambda, got {:?}",
            t.value
        );
    };
    l
}

/// The core claim: a lambda nested inside a weave conditional branch gets a
/// stamped `container_id`, not just one sitting at the top of a knot/stitch
/// body.
#[test]
fn nested_lambda_gets_a_hir_minted_container_id() {
    let hir = lower_and_stamp(SOURCE);
    let l = nested_lambda(&hir);
    assert!(
        l.container_id.is_some(),
        "expected a lambda id to be stamped inside a weave conditional \
         branch, got None — `hir::stamp_container_ids` must walk \
         expressions (TempDecl/Assignment/Return/ExprStmt/LogicBlock/…), \
         not just the branch/gather/choice container ids it already tracked"
    );
}

/// The FG-4d history-independence claim this identity scheme exists to
/// satisfy (`lir::lower::lambda`'s own module doc): stamping the *same*
/// source twice, independently, mints the *same* id both times — a lambda's
/// identity is a pure function of its structural position, never an
/// allocation-order counter.
#[test]
fn nested_lambda_id_is_deterministic_across_independent_stamping_runs() {
    let first = nested_lambda(&lower_and_stamp(SOURCE)).container_id;
    let second = nested_lambda(&lower_and_stamp(SOURCE)).container_id;
    assert!(first.is_some(), "first stamping run produced no id");
    assert_eq!(
        first, second,
        "the same lambda, in the same structural position, must mint the \
         same id on every independent stamping run"
    );
}

/// End-to-end: `lir::lower_to_program_with_type_mode` (the real compile
/// path, which calls `hir::stamp_container_ids` internally via
/// `lir::build_prelude`) assembles a lifted container for the nested lambda
/// whose `DefinitionId` is exactly the id `hir::stamp_container_ids` stamps
/// on `LambdaExpr::container_id` — proving HIR mint and LIR consumption are
/// the *same* value, not just independently non-`None`.
#[test]
fn lifted_container_id_matches_the_hir_stamped_id() {
    let parsed = brink_syntax_native::parse(SOURCE);
    assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
    let file_id = FileId(0);
    let (mut hir, manifest, diags) = lower_native::lower(file_id, &parsed.tree());
    assert!(diags.is_empty(), "{diags:?}");
    brink_ir::hir::normalize_file(&mut hir);

    let files_for_analysis: Vec<(FileId, &HirFile, &brink_ir::SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    let (program, diags) = brink_ir::lir::lower_to_program_with_type_mode(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &HashMap::new(),
        brink_ir::lir::TypeMode::Gradual,
        brink_ir::lir::AnalyzerTables {
            ufcs: &brink_ir::lir::UfcsLookup::new(),
            coalesce: &brink_ir::lir::CoalesceLookup::new(),
        },
    );
    assert!(
        diags.is_empty(),
        "unexpected LIR lowering diagnostics: {diags:?}"
    );
    let program = program.expect("lowering stays total");

    // The stamped id, computed independently over an equivalently
    // parsed+analyzed copy of the same source.
    let stamped_id = nested_lambda(&lower_and_stamp(SOURCE))
        .container_id
        .expect("the nested lambda must be stamped");

    let lifted = find_any(&program.root, &|c| c.id == stamped_id).unwrap_or_else(|| {
        panic!(
            "no assembled container carries the HIR-stamped id {stamped_id:?} — \
             `lir::lower::lambda::lower_lambda` must read \
             `LambdaExpr::container_id` rather than re-deriving its own"
        )
    });
    assert!(
        lifted.is_function,
        "the lifted container for a lambda must be function-shaped"
    );
    assert!(
        lifted
            .name
            .as_deref()
            .is_some_and(|n| n.contains("#lambda-")),
        "the lifted container's name should carry the `#lambda-` marker, got {:?}",
        lifted.name
    );
}

fn find_any<'a>(
    container: &'a brink_ir::lir::Container,
    pred: &dyn Fn(&brink_ir::lir::Container) -> bool,
) -> Option<&'a brink_ir::lir::Container> {
    if pred(container) {
        return Some(container);
    }
    for child in &container.children {
        if let Some(found) = find_any(child, pred) {
            return Some(found);
        }
    }
    None
}
