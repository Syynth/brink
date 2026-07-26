//! B1 `or`-coalescing typing side channel (issue #1492; RULED 2026-07-26,
//! `docs/decision-log.md` "Lowering consumes analyzer types, never
//! re-derives").
//!
//! These fixtures drive the **published seam** — `brink_analyzer::
//! coalesce_types` — rather than the module-internal pass, because the
//! seam is the contract LIR lowering will consume: it must be reachable,
//! keyed the way `brink_ir::hir::expr_span` keys it, and carry a shape
//! verdict for the operand kinds a syntactic shape-sniff in lowering
//! cannot see through (a call's return type, an `Option`-typed local).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{CoalesceChain, CoalesceShape, CoalesceTable};
use brink_ir::hir::lower_native;
use brink_ir::{FileId, HirFile, SymbolManifest};

fn lower(src: &str) -> (HirFile, SymbolManifest) {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (hir, manifest, diags) = lower_native::lower(FileId(0), &parse.tree());
    assert!(diags.is_empty(), "lowering diagnostics: {diags:?}");
    (hir, manifest)
}

/// The recorded coalescing table for one `.brink` file, through the public
/// seam exactly as a consumer would reach it.
fn table(src: &str) -> CoalesceTable {
    let (hir, manifest) = lower(src);
    let files = vec![(FileId(0), &hir, &manifest)];
    let analysis = brink_analyzer::analyze(&files);
    let hir_inputs = vec![(FileId(0), &hir)];
    let manifest_inputs = vec![(FileId(0), &manifest)];
    let inline_docs = brink_analyzer::project_inline_docs(&manifest_inputs);
    let inference = brink_analyzer::infer_project(
        &hir_inputs,
        &analysis.index,
        &analysis.resolutions,
        None,
        &inline_docs,
    );
    let (table, _diags) = brink_analyzer::coalesce_types(
        &hir_inputs,
        &analysis.index,
        &inference,
        &analysis.resolutions,
    );
    table
}

fn only_chain(src: &str) -> CoalesceChain {
    let table = table(src);
    assert_eq!(table.len(), 1, "expected exactly one chain: {table:?}");
    let (_key, chain) = table.iter().next().unwrap();
    chain.clone()
}

/// The verdict PR #1479's `rhs_is_option_shaped` heuristic could not
/// reach: the fallback is an `Expr::Call`, whose `Option`-ness lives in the
/// callee's inferred return type, not in the call's syntax.
#[test]
fn an_option_returning_call_fallback_is_recorded_as_option_preserving() {
    let chain = only_chain(concat!(
        "fn maybe() {\n  return some(7);\n}\n",
        "flow main() {\n  {some(1) or maybe()}\n  -> END\n}\n",
    ));
    assert_eq!(chain.steps.len(), 1);
    assert_eq!(chain.steps[0].shape, CoalesceShape::PreserveOption);
}

/// `some(5) or maybe() or 99` — the exact chain named in the #1492 fence.
/// The inner step keeps optionality (so the outer step has an `Option` to
/// coalesce), and only the trailing plain fallback collapses.
#[test]
fn the_blocking_chain_records_preserve_then_collapse() {
    let chain = only_chain(concat!(
        "fn maybe() {\n  return some(7);\n}\n",
        "flow main() {\n  {some(5) or maybe() or 99}\n  -> END\n}\n",
    ));
    let shapes: Vec<CoalesceShape> = chain.steps.iter().map(|s| s.shape).collect();
    assert_eq!(
        shapes,
        vec![CoalesceShape::PreserveOption, CoalesceShape::Collapse],
        "innermost first: {chain:?}"
    );
}

/// The w56 scope finding folded into #1492: a bare `Path` fallback whose
/// binding holds an `Option`. Syntax says "an identifier"; the recorded
/// type says `Option[int]`.
#[test]
fn an_option_typed_local_fallback_is_recorded_as_option_preserving() {
    let chain = only_chain(concat!(
        "fn pick() {\n",
        "  let fallback = some(3);\n",
        "  return some(1) or fallback;\n",
        "}\n",
        "flow main() {\n  -> END\n}\n",
    ));
    assert_eq!(chain.steps.len(), 1);
    assert_eq!(chain.steps[0].shape, CoalesceShape::PreserveOption);
}

/// A plain fallback collapses — the other arm of the same decision, so the
/// test above cannot pass by the verdict being constant.
#[test]
fn a_plain_fallback_is_recorded_as_collapsing() {
    let chain = only_chain("flow main() {\n  {some(1) or 2}\n  -> END\n}\n");
    assert_eq!(chain.steps.len(), 1);
    assert_eq!(chain.steps[0].shape, CoalesceShape::Collapse);
}

/// An ill-typed chain is rejected at analysis (`E066`) and hands lowering
/// nothing — "an ill-typed chain never reaches lowering".
#[test]
fn an_ill_typed_chain_is_served_no_verdict() {
    assert!(table("flow main() {\n  {some(1) or \"text\"}\n  -> END\n}\n").is_empty());
}

/// A file with no coalescing at all costs nothing and records nothing —
/// including every ink-dialect project, which cannot produce
/// `InfixOp::Coalesce` at all.
#[test]
fn a_project_without_coalescing_records_nothing() {
    assert!(table("flow main() {\n  Hello.\n  -> END\n}\n").is_empty());
}

/// The key a consumer looks the verdict up under is `expr_span` of the
/// chain root, in `brink-ir` — the derivation LIR lowering shares. Proven
/// here rather than assumed, because a producer/consumer disagreement
/// about the key is silent: the lookup just misses.
#[test]
fn the_key_is_expr_span_of_the_chain_root() {
    let src = "flow main() {\n  {some(1) or 2}\n  -> END\n}\n";
    let (hir, manifest) = lower(src);
    let recorded = {
        let files = vec![(FileId(0), &hir, &manifest)];
        let analysis = brink_analyzer::analyze(&files);
        let hir_inputs = vec![(FileId(0), &hir)];
        let inline_docs = brink_analyzer::project_inline_docs(&[(FileId(0), &manifest)]);
        let inference = brink_analyzer::infer_project(
            &hir_inputs,
            &analysis.index,
            &analysis.resolutions,
            None,
            &inline_docs,
        );
        brink_analyzer::coalesce_types(
            &hir_inputs,
            &analysis.index,
            &inference,
            &analysis.resolutions,
        )
        .0
    };

    // Reconstruct the key the way a consumer holding the HIR node does.
    let root = find_coalesce(&hir).expect("the fixture has one coalescing expression");
    let range = brink_ir::hir::expr_span(root).expect("the root spans `some(1)`");
    assert!(
        recorded.at(FileId(0), range).is_some(),
        "consumer-side key missed: {recorded:?}"
    );
}

/// The first coalescing expression in a file's knot bodies.
fn find_coalesce(hir: &HirFile) -> Option<&brink_ir::Expr> {
    struct Finder<'a> {
        found: Option<&'a brink_ir::Expr>,
    }
    // `visit`'s callbacks are `&Expr` with the walk's lifetime, so the
    // search is a plain hand recursion over the knot bodies instead.
    fn scan<'a>(stmts: &'a [brink_ir::Stmt], out: &mut Finder<'a>) {
        for stmt in stmts {
            if let brink_ir::Stmt::Content(c) = stmt {
                for part in &c.parts {
                    if let brink_ir::ContentPart::Interpolation(e) = part
                        && matches!(e, brink_ir::Expr::Infix(_, brink_ir::InfixOp::Coalesce, _))
                        && out.found.is_none()
                    {
                        out.found = Some(e);
                    }
                }
            }
        }
    }
    let mut finder = Finder { found: None };
    for knot in &hir.knots {
        scan(&knot.body.stmts, &mut finder);
    }
    finder.found
}
