//! B3a UFCS **auto-ref** lowering (D5, issue #1462): a free function whose
//! first parameter is declared `ref` receives its UFCS receiver *by
//! reference*, spelled as the explicit projection the free-call form would
//! use (`party.leader.heal(5)` → `heal(ref party.leader, 5)`).
//!
//! The end-to-end proof that this compiles, plays, and leaves the mutation
//! visible in the caller lives in `brink-test-harness/tests/b3a_ufcs_e2e.rs`
//! (`auto_ref_mutates_a_local_receiver_end_to_end` and its global-`VAR`
//! twin). What that file *cannot* reach is the **projection** shape: a T1e
//! projection's root must be a durable cell (`docs/t1e-spec.md` §2), and the
//! native surface has no way to spell a struct-typed global today — a
//! construction literal is refused as a `VAR` default (`E075`) and a
//! struct-typed declaration derives no type in `infer::collect_globals`
//! (`InferredType` has no struct form), so the analyzer answers `E142`
//! before auto-ref is ever consulted. This file therefore drives the
//! lowering with a hand-assembled verdict table, mirroring the workaround
//! `ufcs_field_call.rs` documents for the same class of gap, so the arm is
//! covered rather than assumed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Issue #801: this crate's `clippy.toml` disallows bare `HashMap`/`HashSet`
// so an iteration-order leak into codegen output can't ship quietly. This
// file's one use (an always-empty `file_paths` map handed to
// `lower_to_program_with_type_mode`) has no order to leak — the same
// exemption `ufcs_field_call.rs` and `tests/lir_lowering.rs` take.
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map, no order to leak — see file doc"
)]

use brink_format::DefinitionId;
use brink_ir::hir::lower_native;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{Expr, FileId, HirFile, SymbolKind, lir};
use rowan::TextRange;

/// The `(file, range) → verdict` table the analyzer would publish, built by
/// hand: the one multi-segment call site in `src`, resolved to the free
/// function `target_name` as an auto-ref desugar.
fn auto_ref_lookup(hir: &HirFile, target: DefinitionId) -> lir::UfcsLookup {
    lir::UfcsLookup::from_entries(vec![(
        FileId(0),
        ufcs_call_range(hir),
        lir::UfcsVerdict::FreeFnAutoRef { target },
    )])
}

/// The single multi-segment callee path's range in `hir`.
fn ufcs_call_range(hir: &HirFile) -> TextRange {
    struct Scan {
        found: Vec<TextRange>,
    }
    impl HirVisitor for Scan {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_expr(&mut self, expr: &Expr) {
            if let Expr::Call(path, _) = expr
                && path.segments.len() > 1
            {
                self.found.push(path.range);
            }
        }
    }
    let mut scan = Scan { found: Vec::new() };
    visit::visit(hir, &mut scan);
    assert_eq!(scan.found.len(), 1, "one UFCS site: {:?}", scan.found);
    scan.found[0]
}

/// Parse, lower, resolve and LIR-lower `src`, returning the argument row of
/// the one call in `main`'s body.
fn lower_call_args(src: &str) -> Vec<lir::CallArg> {
    let parsed = brink_syntax_native::parse(src);
    assert!(
        parsed.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let file_id = FileId(0);
    let (hir, manifest, lower_diags) = lower_native::lower(file_id, &parsed.tree());
    assert!(
        lower_diags.is_empty(),
        "lowering diagnostics: {lower_diags:?}"
    );

    // Index + resolutions only: the *analysis* verdict is supplied by hand
    // below (see the module doc), so the type-directed pass is deliberately
    // not run here.
    let (index, _index_diags) = brink_analyzer::symbol_index(&[(file_id, &manifest)]);
    let scope =
        brink_analyzer::ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
    let (file_resolutions, _resolve_diags) =
        brink_analyzer::resolve(file_id, &manifest, &index, &scope);
    let mut resolutions = brink_analyzer::ResolutionMap::new();
    resolutions.extend(std::sync::Arc::unwrap_or_clone(file_resolutions));

    let target = index
        .symbols
        .iter()
        .find(|(_, info)| info.kind == SymbolKind::Knot && info.name == "heal")
        .map(|(id, _)| *id)
        .expect("the free function `heal`");
    assert!(
        index.symbols[&target].params[0].is_ref,
        "the fixture's first parameter must be `ref`"
    );

    let ufcs = auto_ref_lookup(&hir, target);
    let hir_inputs = vec![(file_id, &hir)];
    let (program, lir_diags) = lir::lower_to_program_with_type_mode(
        &hir_inputs,
        &index,
        &resolutions,
        &std::collections::HashMap::new(),
        lir::TypeMode::Gradual,
        lir::AnalyzerTables {
            ufcs: &ufcs,
            coalesce: &lir::CoalesceLookup::new(),
        },
    );
    assert!(
        lir_diags.is_empty(),
        "LIR lowering diagnostics: {lir_diags:?}"
    );
    let program = program.expect("lower_to_program_with_type_mode is total");

    let main_knot = program
        .root
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some("main"))
        .expect("a `main` knot container");
    // `lir::Stmt`/`Expr` derive neither `Debug` nor `PartialEq`, so this
    // walks and matches by hand rather than via `assert_eq!` — same as
    // `ufcs_field_call.rs`.
    main_knot
        .body
        .iter()
        .find_map(|stmt| match stmt {
            lir::Stmt::ExprStmt(lir::Expr::Call { args, .. }) => Some(args.clone()),
            _ => None,
        })
        .expect("a call statement in `main`'s body")
}

/// A **bare** receiver auto-refs exactly like an unmarked ref-argument
/// always has: a pointer to the global cell, not a copy of its value.
#[test]
fn a_bare_global_receiver_auto_refs_to_the_global_cell() {
    let args = lower_call_args(
        "\
var party = 0

fn heal(ref h, amount) {
  h = h + amount;
}

fn main() {
  party.heal(5);
}
",
    );
    assert_eq!(args.len(), 2, "receiver + one written argument");
    assert!(
        matches!(args[0], lir::CallArg::RefGlobal(_)),
        "the receiver must be passed by reference, not by value"
    );
    assert!(
        matches!(args[1], lir::CallArg::Value(lir::Expr::Int(5))),
        "the written argument is unaffected by auto-ref"
    );
}

/// A **dotted** receiver becomes a real T1e projection over the durable
/// root: `party.leader.heal(5)` → `heal(ref party.leader, 5)`, with the
/// field segment spelled explicitly.
#[test]
fn a_dotted_receiver_auto_refs_as_an_explicit_projection() {
    let args = lower_call_args(
        "\
var party = 0

fn heal(ref h, amount) {
  h = h + amount;
}

fn main() {
  party.leader.heal(5);
}
",
    );
    assert_eq!(args.len(), 2, "receiver + one written argument");
    let lir::CallArg::RefProjection { segments, .. } = &args[0] else {
        panic!("the receiver must lower as a T1e ref projection");
    };
    assert_eq!(segments.len(), 1, "one field segment (`leader`)");
    let lir::Expr::String(spelled) = &segments[0] else {
        panic!("a field segment lowers to a literal string expression");
    };
    assert!(
        matches!(
            spelled.parts.as_slice(),
            [lir::StringPart::Literal(name)] if name == "leader"
        ),
        "the projection must name the field it walks"
    );
}
