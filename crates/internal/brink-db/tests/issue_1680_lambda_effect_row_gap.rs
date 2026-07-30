//! Issue #1680, characterization: a **lambda-lifted function has no row in
//! the `EffectRows` table at all**, so `effects-spec.md` §7's "a live fn
//! value is a token; its row is a table lookup" cannot resolve for the one
//! value form the native higher-order core is built on.
//!
//! This is a *gap* pin, not a contract — it asserts what the compiler does
//! today so the next attempt at the shipped-table/§7-narrowing path (§6
//! item 4, an optional host optimization, and conditionally T1c item 4's
//! row field) finds the gap already measured instead of rediscovering it.
//! It does not block #1680's own analyzer-side work (rows on `Ty::Fn`, the
//! unifier row join, §6.1 row-polymorphism). It is the same discipline
//! #1685 (flipped by #1709) used for its `E052` fence, and it must
//! **flip** when the row table is made to reach lifted lambdas.
//!
//! ## Why the row is missing
//!
//! The obstacle is the keyspace, not the order the id and the rows are
//! minted in — by the time `populate_effect_rows` runs, the lambda's
//! `DefinitionId` already exists (`story_data_query` orders
//! `lir_in_closure_query` → `brink_codegen_inkb::emit` →
//! `populate_effect_rows`, so LIR lowering, which mints the id, has already
//! completed):
//!
//! - `populate_effect_rows` (`brink-db/src/queries/mod.rs`) walks
//!   `inferable_defs_query`, which is
//!   `brink_analyzer::inferable_defs_from_index` — `index.symbols` filtered
//!   to `SymbolKind::Knot | SymbolKind::Stitch`
//!   (`brink-analyzer/src/infer/mod.rs`). A lambda is an inline
//!   `hir::Expr::Lambda`, never an indexed knot/stitch symbol, so no
//!   iteration of that set can ever yield one.
//! - The lifted function's `DefinitionId` is minted in **LIR** lowering, by
//!   `IdAllocator::alloc_lambda_address` (`brink-ir/src/lir/lower/context.rs`),
//!   but a lambda has no index symbol, so it has no `DefKey`/SCC membership
//!   and `inferable_defs_from_index` was never going to enumerate it
//!   regardless of when the id is minted.
//!
//! So the id that ends up in a live `VAL_FN_REF`/`VAL_CLOSURE` token is a
//! `DefinitionTag::Address` id that the row table was never given a chance
//! to key.
//!
//! ## What is *not* broken
//!
//! Soundness. `InferPass::infer_lambda` (`brink-analyzer/src/infer/body.rs`)
//! walks the lambda body's statements and value expression inside the
//! **enclosing** definition's pass, so every atom the body performs is
//! absorbed into the enclosing def's row. That over-reports (spec §3's
//! conservative-total direction — over-report is always allowed) rather
//! than under-reporting. The second test below pins that for an
//! expression-bodied lambda (`|x| expr`); the third pins the same claim for
//! a **block**-bodied lambda (`|x|: T { stmts…; tail }`) whose read lives in
//! `stmts`, not `tail` — issue #1749 found that `infer_lambda` originally
//! walked only `LambdaBody::value_exprs()` (the tail alone), so a
//! block-bodied lambda's own statements were silently never absorbed. Both
//! tests must keep passing, so a future precision change cannot quietly
//! drop the absorption while making the first test pass.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, Dialect, TypePolicy};
use brink_db::ProjectDb;
use brink_format::DefinitionId;
use brink_ir::DiagnosticCode;

/// Strict-mode options for the native (`.brink`) dialect — the same shape
/// `tm3_strict.rs` uses to reach TM-3's diagnostics through the production
/// `db.diagnostics(file)` seam.
fn strict_native_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    }
}

/// A native project whose only read of `counter` happens *inside* a lambda,
/// which is then lifted to its own top-level function by #1709.
const SOURCE: &str = "\
var counter = 7

fn bumped(n: int): int {
  let f = |x| x + counter;
  return f(n);
}

flow main() {
  Bumped: {bumped(3)}
  -> END
}
";

/// Compile `SOURCE` and hand back the linked story plus the ids of every
/// lambda-lifted container in it.
///
/// Lifted containers are found through `address_paths`: lifting names its
/// synthesized function `{enclosing scope path}.#lambda-{source start
/// offset}` (`brink-ir/src/lir/lower/lambda.rs`), and `#lambda-` is not a
/// spelling any author path can produce — `#` cannot start a native path
/// segment.
fn lifted_lambda_defs(story: &brink_format::StoryData) -> Vec<DefinitionId> {
    story
        .address_paths
        .iter()
        .filter(|ap| {
            story
                .name_table
                .get(ap.path.0 as usize)
                .is_some_and(|p| p.contains("#lambda-"))
        })
        .map(|ap| ap.target)
        .collect()
}

/// The gap itself: the lifted function is a real, addressable container in
/// the story, and the `EffectRows` table has nothing for it.
#[test]
fn a_lifted_lambda_ships_no_effect_row() {
    let mut db = ProjectDb::new();
    db.set_file("main.brink", SOURCE.to_owned());
    db.set_entry("main.brink");

    let product = db.story_data().expect("entry is set");
    let story = product.story.as_ref().expect("story compiles cleanly");

    // Guard the guard: if lifting ever stops emitting an addressable
    // container the filter below would vacuously pass.
    let lifted = lifted_lambda_defs(story);
    assert!(
        !lifted.is_empty(),
        "the lambda must lift to an addressable container (#1709); \
         name_table = {:?}",
        story.name_table
    );

    assert!(
        !story.effect_rows.is_empty(),
        "the story's knots/stitches do ship rows, so an empty table would \
         make the next assertion meaningless"
    );

    let with_rows: Vec<DefinitionId> = story.effect_rows.iter().map(|r| r.def).collect();
    for def in &lifted {
        assert!(
            !with_rows.contains(def),
            "#1680: a lambda-lifted function is expected to carry no \
             `EffectRows` entry today — if this now fails, the row table \
             reaches lifted lambdas and this characterization must be \
             replaced by the real contract"
        );
    }
}

/// The soundness half: the lambda's read of `counter` is absorbed into the
/// **enclosing** definition's row, so nothing under-reports while the lifted
/// function's own row is missing (spec §3, conservative-total).
#[test]
fn the_enclosing_def_absorbs_the_lambda_bodys_atoms() {
    let mut db = ProjectDb::new();
    db.set_file("main.brink", SOURCE.to_owned());
    db.set_entry("main.brink");

    let index = db.symbol_index();
    let bumped = *index
        .by_name
        .get("bumped")
        .expect("`bumped` is indexed")
        .first()
        .expect("indexed name has at least one def");
    let counter = *index
        .by_name
        .get("counter")
        .expect("`counter` is indexed")
        .first()
        .expect("indexed name has at least one def");

    let product = db.story_data().expect("entry is set");
    let story = product.story.as_ref().expect("story compiles cleanly");

    let row = story
        .effect_rows
        .iter()
        .find(|r| r.def == bumped)
        .expect("`bumped` ships a container row");

    assert!(
        row.direct.reads.contains(&counter),
        "the lambda body's read of `counter` must be absorbed into the \
         enclosing def's row — reads = {:?}",
        row.direct.reads
    );
}

/// A native project whose block-bodied lambda's *statement* (not its tail)
/// reads `counter` — the exact shape issue #1749 found `infer_lambda`
/// dropping. `infer_lambda` originally called only
/// `LambdaBody::value_exprs()`, which for `LambdaBody::Block` yields the
/// tail alone, so this statement's read was silently never absorbed into
/// the enclosing def's row.
const SOURCE_BLOCK_BODY: &str = "\
var counter = 7

fn bumped_block(n: int): int {
  let f = |x|: int {
    let a = x + counter;
    a
  };
  return f(n);
}

flow main() {
  BumpedBlock: {bumped_block(3)}
  -> END
}
";

/// The block-bodied twin of
/// `the_enclosing_def_absorbs_the_lambda_bodys_atoms` (issue #1749): the
/// read of `counter` lives in the lambda's `stmts`, not its `tail`, so this
/// is the one shape `value_exprs()` alone could never surface — the
/// narrower assertion the original characterization test carried only
/// proved the expression-bodied case.
#[test]
fn the_enclosing_def_absorbs_a_block_bodied_lambdas_stmt_atoms() {
    let mut db = ProjectDb::new();
    db.set_file("main.brink", SOURCE_BLOCK_BODY.to_owned());
    db.set_entry("main.brink");

    let index = db.symbol_index();
    let bumped = *index
        .by_name
        .get("bumped_block")
        .expect("`bumped_block` is indexed")
        .first()
        .expect("indexed name has at least one def");
    let counter = *index
        .by_name
        .get("counter")
        .expect("`counter` is indexed")
        .first()
        .expect("indexed name has at least one def");

    let product = db.story_data().expect("entry is set");
    let story = product.story.as_ref().expect("story compiles cleanly");

    let row = story
        .effect_rows
        .iter()
        .find(|r| r.def == bumped)
        .expect("`bumped_block` ships a container row");

    assert!(
        row.direct.reads.contains(&counter),
        "the block-bodied lambda's *stmt* read of `counter` (not its tail) \
         must be absorbed into the enclosing def's row — reads = {:?}",
        row.direct.reads
    );
}

// ── Post-#1749-review regressions: the `stmts` walk above reuses the
// enclosing def's own `infer_block_stmt`, which also mutates frame-scoped
// bookkeeping (`return_ty`, `has_value_return`, `locals`, `annotated`,
// `local_fn_origins`) that belongs to the lambda's own, separate frame.
// The characterization test above only exercises one arm (`TempDecl` whose
// initializer *reads* a global) — the three tests below pin the arms that
// regressed: `Return`, a bare call `ExprStmt`, and a `TempDecl` whose
// *binding* collides by name with an outer local.

/// A block-bodied lambda's own `return` must not be attributed to the
/// ENCLOSING definition's return-type bookkeeping. Before the fix,
/// `BlockStmt::Return` → `infer_return` set `self.has_value_return = true`
/// and joined into `self.return_ty` on whatever frame `infer_block_stmt`
/// was called against — the enclosing one, since `infer_lambda` reused it
/// unchanged — silently satisfying (and hence swallowing) this def's own
/// `E150` (declares a return type but its body never returns a value).
const SOURCE_LAMBDA_RETURN_LEAK: &str = "\
fn e150_lambda_return_leak(n: int): int {
  let g = || {
    return 1;
  };
}
";

#[test]
fn a_lambdas_return_does_not_satisfy_the_enclosing_defs_e150_check() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", SOURCE_LAMBDA_RETURN_LEAK.to_owned());
    db.set_entry("main.brink");
    db.set_analysis_options(strict_native_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E150),
        "`e150_lambda_return_leak` itself never returns a value — the \
         lambda's own `return 1` must not satisfy this def's E150 check: \
         {diags:?}"
    );
}

/// A bare call `ExprStmt` (not a `TempDecl` initializer) inside a
/// block-bodied lambda's `stmts` must still resolve a call-graph edge from
/// the ENCLOSING def, so the effect fixpoint pulls the callee's own row in
/// transitively — the shape the characterization test above never
/// exercises (it only reads a global directly from a `TempDecl`
/// initializer).
const SOURCE_BLOCK_CALL_STMT: &str = "\
var flag = false

fn mark(x: int): int {
  flag = true;
  return x;
}

fn bumped_call(n: int): int {
  let f = |x|: int {
    mark(x);
    x
  };
  return f(n);
}

flow main() {
  BumpedCall: {bumped_call(3)}
  -> END
}
";

#[test]
fn the_enclosing_def_absorbs_a_block_bodied_lambdas_call_stmt_atom() {
    let mut db = ProjectDb::new();
    db.set_file("main.brink", SOURCE_BLOCK_CALL_STMT.to_owned());
    db.set_entry("main.brink");

    let index = db.symbol_index();
    let bumped_call = *index
        .by_name
        .get("bumped_call")
        .expect("`bumped_call` is indexed")
        .first()
        .expect("indexed name has at least one def");
    let flag = *index
        .by_name
        .get("flag")
        .expect("`flag` is indexed")
        .first()
        .expect("indexed name has at least one def");

    let product = db.story_data().expect("entry is set");
    let story = product.story.as_ref().expect("story compiles cleanly");

    let row = story
        .effect_rows
        .iter()
        .find(|r| r.def == bumped_call)
        .expect("`bumped_call` ships a container row");

    assert!(
        row.direct.writes.contains(&flag),
        "the block-bodied lambda's `mark(x)` call statement (a bare \
         ExprStmt, not a TempDecl initializer) must resolve a call-graph \
         edge so the fixpoint pulls `mark`'s write of `flag` into the \
         enclosing def's row — writes = {:?}",
        row.direct.writes
    );
}

/// A block-bodied lambda's own `let` must not corrupt an outer local of the
/// same name. Before the fix, `BlockStmt::TempDecl`'s `bind_local` called
/// `unify` against whatever `self.locals` entry already existed for that
/// name — the enclosing def's own `a`, since capture is by value and the
/// lambda's `a` is a wholly separate binding — joining `string` into the
/// enclosing `a`'s `int` and turning the enclosing def's own return type
/// `Conflicted` (spurious `E066`), even though the lambda's `a` never
/// escapes it.
const SOURCE_OUTER_TEMP_SHADOW: &str = "\
fn shadow(n: int): int {
  let a = n + 1;
  let g = |x: int|: string {
    let a = \"str\";
    a
  };
  return a;
}
";

#[test]
fn a_lambdas_temp_does_not_corrupt_an_outer_same_named_local() {
    let mut db = ProjectDb::new();
    let file = db.set_file("main.brink", SOURCE_OUTER_TEMP_SHADOW.to_owned());
    db.set_entry("main.brink");
    db.set_analysis_options(strict_native_opts());

    let diags = db.diagnostics(file).expect("file diagnostics");
    assert!(
        diags.is_empty(),
        "the lambda's own `let a = \"str\"` is a separate binding from the \
         enclosing `let a = n + 1` — it must not unify into the enclosing \
         local and conflict the enclosing def's own return type: {diags:?}"
    );
}
