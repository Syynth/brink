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
//!
//! ## The #1504-interplay gap (review finding on this issue's own PR)
//!
//! The three tests above all pass an **empty** `file_paths` map, so every
//! `IdAllocator` path prefix (#1504's per-file qualifier,
//! `hir::root_content_scope_path`) stays empty and `lower_lambda`'s
//! `scope_path: path.clone()` (handing the *already-prefix-qualified* path
//! down as the child `LowerCtx`'s scope, instead of the unqualified
//! `relative`) was a silent no-op bug: with no prefix, qualifying twice is
//! the same as qualifying once. [`nested_decl_lambda_matches_across_stamped_
//! and_unstamped_paths`] below registers a real (non-empty) file path AND
//! nests a lambda inside another lambda's own body, in a file-scope
//! `CONST` default (issue #1774's `decls::eval_const_lambda` path) — the one
//! shape where the *unstamped* projection `brink-db`'s `decl_hir_query`
//! deliberately builds (see that query's doc) drives `lower_lambda`'s
//! id-mint fallback for a **nested** lambda, rather than reading a
//! pre-stamped `container_id`. Before the fix, that fallback re-applied the
//! path prefix a second time for the inner lambda, minting a different
//! `DefinitionId` than the whole-project stamped walk — the exact FG-4d
//! history-independence violation this issue exists to prevent. Verified to
//! fail without the fix: temporarily reverted `lower_lambda`'s
//! `scope_path: relative.clone()` back to `scope_path: path.clone()` and
//! re-ran this test — it failed with "the unstamped decl-hir-query path's
//! inner lambda must mint the SAME `DefinitionId` as the whole-project
//! stamped walk", then restored and re-verified green.

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
use brink_ir::{BlockStmt, Expr, FileId, HirFile, LambdaBody, LambdaExpr, Stmt, lir};

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

// ─── The #1504-interplay gap ──────────────────────────────────────────

/// A file-scope `const` whose default is a lambda literal with another
/// lambda nested inside its own braced block body (issue #1774's
/// decl-default lambda path, `decls::eval_const_lambda`, crossed with
/// #1727's own nested-lambda shape). Unlike [`SOURCE`] above, this fixture
/// is always lowered with a REAL, non-empty registered file path — see the
/// module doc's "#1504-interplay gap" section for why that is load-bearing.
const NESTED_DECL_SOURCE: &str = "const f = |a| { let g = |b| b; g(a) }\n";

/// The registered project path for [`NESTED_DECL_SOURCE`] — any non-empty
/// value exercises `IdAllocator`'s per-file prefix (#1504); the exact
/// spelling is not load-bearing.
const NESTED_DECL_FILE_PATH: &str = "story/globals.brink";

/// Navigate to the `f` decl's lambda default.
fn nested_decl_lambda(hir: &HirFile) -> &LambdaExpr {
    let cst = hir.constants.first().expect("expected one CONST decl");
    let Expr::Lambda(l) = &cst.value else {
        panic!(
            "expected `f`'s default to be a lambda literal, got {:?}",
            cst.value
        );
    };
    l
}

/// Navigate from the outer lambda to the `g` lambda nested in its block
/// body's first statement (`let g = |b| b;`).
fn inner_lambda(outer: &LambdaExpr) -> &LambdaExpr {
    let LambdaBody::Block { stmts, .. } = &outer.body else {
        panic!(
            "expected `f`'s body to be a braced block, got {:?}",
            outer.body
        );
    };
    let BlockStmt::TempDecl(t) = stmts.first().expect("expected the `let g = …` statement")
    else {
        panic!(
            "expected the block's first statement to be a TempDecl, got {:?}",
            stmts.first()
        );
    };
    let Some(Expr::Lambda(l)) = t.value.as_ref() else {
        panic!(
            "expected `g`'s default to be a lambda literal, got {:?}",
            t.value
        );
    };
    l
}

/// The review finding's own repro: with a REAL registered file path, a
/// lambda nested inside a decl-default lambda's body must mint the *same*
/// `DefinitionId` whether it is reached through the whole-project stamped
/// walk (`hir::stamp_container_ids` + the ordinary `lower_lambda` read of
/// `container_id`) or through brink-db's deliberately UNSTAMPED
/// `decl_hir_query` projection lowered via `lir::build_prelude_decls`
/// directly (the same function `lir_prelude_decls_query` calls) — which
/// forces `lower_lambda`'s id-mint fallback to engage for both the outer
/// and the inner lambda, since neither ever got a stamped `container_id`.
#[test]
fn nested_decl_lambda_matches_across_stamped_and_unstamped_paths() {
    let file_id = FileId(0);

    // The raw HIR, exactly as `decl_hir_query` hands it to `collect_globals`
    // — never normalized, never stamped (that query clones straight off
    // `lowered_query`'s output; see its own doc).
    let parsed = brink_syntax_native::parse(NESTED_DECL_SOURCE);
    assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
    let (raw, manifest, diags) = lower_native::lower(file_id, &parsed.tree());
    assert!(
        diags.is_empty(),
        "unexpected lowering diagnostics: {diags:?}"
    );

    // A real analyze pass over the normalized copy — mirrors
    // `resolutions_index_query`, which brink-db computes once, project-wide,
    // off normalized HIR regardless of which per-file projection reads it.
    let mut normalized = raw.clone();
    brink_ir::hir::normalize_file(&mut normalized);
    let files_for_analysis: Vec<(FileId, &HirFile, &brink_ir::SymbolManifest)> =
        vec![(file_id, &normalized, &manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let mut file_paths: HashMap<FileId, String> = HashMap::new();
    file_paths.insert(file_id, NESTED_DECL_FILE_PATH.to_string());

    // ── Reference: the whole-project, fully-stamped walk ──
    let mut slice = [(file_id, normalized.clone())];
    brink_ir::stamp_container_ids(&mut slice, &result.index, &file_paths);
    let [(_, stamped)] = slice;

    let outer_stamped = nested_decl_lambda(&stamped);
    let outer_id = outer_stamped
        .container_id
        .expect("outer decl-default lambda must be HIR-stamped");
    let inner_id = inner_lambda(outer_stamped)
        .container_id
        .expect("nested lambda inside a decl-default lambda's body must also be HIR-stamped");

    // ── The path under test: the UNSTAMPED decl-hir-query projection,
    // lowered directly through `build_prelude_decls` — no root content, no
    // knots, exactly what `lir_prelude_decls_query` feeds it.
    let ufcs = brink_ir::lir::UfcsLookup::new();
    let coalesce = brink_ir::lir::CoalesceLookup::new();
    let tables = brink_ir::lir::AnalyzerTables {
        ufcs: &ufcs,
        coalesce: &coalesce,
    };
    let files: Vec<(FileId, &HirFile)> = vec![(file_id, &raw)];
    let decls = brink_ir::lir::build_prelude_decls(
        &files,
        &result.index,
        &result.resolutions,
        &file_paths,
        brink_ir::lir::TypeMode::Gradual,
        tables,
    );
    let prelude = brink_ir::lir::assemble_prelude(decls, vec![(file_id, raw)]);
    let program = brink_ir::lir::assemble_program(&prelude, Vec::new(), 0, &result.index);

    let g = program
        .globals
        .iter()
        .find(|g| program.name_table[g.name.0 as usize] == "f")
        .expect("no global named `f`");
    let lir::ConstValue::FnRef(outer_target) = g.default else {
        panic!(
            "expected `f`'s decl default to fold to a bare FnRef, got {:?}",
            g.default
        );
    };
    assert_eq!(
        outer_target, outer_id,
        "the unstamped decl-hir-query path's outer lambda must mint the \
         SAME DefinitionId as the whole-project stamped walk"
    );

    let inner_lifted = find_any(&program.root, &|c| c.id == inner_id).unwrap_or_else(|| {
        panic!(
            "no assembled container from the unstamped decl-hir-query path \
             carries the expected nested-lambda id {inner_id:?} (computed \
             independently by the whole-project stamped walk) — \
             `lower_lambda`'s id-mint fallback must hand the child `LowerCtx` \
             the unqualified `relative` scope path, not the already-prefixed \
             `path`, or a nested lambda's fallback id gets the file prefix \
             applied twice"
        )
    });
    assert!(
        inner_lifted.is_function,
        "the nested lambda's lifted container must be function-shaped"
    );
}
