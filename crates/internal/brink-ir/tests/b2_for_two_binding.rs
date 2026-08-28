//! B2 exit-criterion test: `for k, v in m` two-binding map iteration's LIR
//! desugar (issue #1461, docs/stdlib-spec.md §5/§9's F10 ruling — "desugars
//! to key-iteration + `let v = m[k]`, total by construction, no pair shape
//! ever materializes").
//!
//! Lives as an integration test for the same reason `b06_native_declarations.rs`/
//! `b07_native_body.rs`/`b08_native_control_flow.rs` do (see those files'
//! module docs): the full pipeline needs `brink-analyzer`, a dev-dependency
//! that itself depends on `brink-ir`.
//!
//! # Reachability, honestly
//!
//! This drives real native `.brink` source all the way through the
//! production pipeline — `brink_syntax_native::parse` →
//! `hir::lower_native::lower` → `brink_analyzer::analyze_with_options` →
//! `lir::lower_to_program` — the same low-level pipeline shape
//! `brink-test-harness::corpus::compile_and_explore_from_brink_native` runs,
//! stopping one step short of codegen so the test can inspect the
//! `lir::Container` body shape directly instead of only observing "it
//! compiled". **Not** the same analyzer configuration as that harness
//! function any more (issue #1472): this file deliberately hardcodes
//! `dialect: Dialect::Brink` below to reach `TypePolicy::Strict` — see the
//! next paragraph — where the harness's fixed version now passes
//! `dialect: StrictInk` (its real default) with `is_native: true`, since
//! `resolve_type_policy` has no `is_native` input of its own to force
//! strict typing by (a gap this issue's investigation surfaced and flagged,
//! not fixed).
//!
//! The iterable (`m`) is an untyped `fn` parameter, not a real map value:
//! the native surface has no map (or array) literal grammar yet (B5,
//! `TypeName { … }` construction, #1103, is unfiled), and no type-
//! annotation syntax for parameters either — so there is currently no way
//! to construct, or even *declare the type of*, a real collection from
//! `.brink` source at all. Native is *meant* to be strict-typed
//! unconditionally (docs/decision-log.md "Typing posture ruled": "gradual
//! typing does not exist on the native surface") but nothing keys that off
//! `is_native` today — only `dialect == Brink` resolves to
//! `TypePolicy::Strict` (issue #1127), so this fixture reaches it by
//! hardcoding `dialect: Dialect::Brink` directly rather than through any
//! native-specific policy. So an `Unknown`-typed loop binding is a hard
//! `E065` ("escapes strict inference as Unknown"),
//! not a warning. Concretely: **`for k, v in m` cannot be written
//! analysis-clean anywhere on the native surface today** — not because of
//! anything this change does, but because nothing on the native surface
//! can give `m` a map type yet. This is the honest, confirmed shape of the
//! prerequisite gap (filed as a scope note on issue #1461, alongside B5)
//! — the existing `b08_native_control_flow.rs` single-binding coverage has
//! the same limit one level down (it proves shape, not a real run over
//! real data, but at least stays analysis-clean by using an *unresolved*
//! path rather than a typed parameter).
//!
//! So this test accepts the exact, expected `E065` diagnostics (documented
//! per assertion below) rather than requiring zero — and still drives the
//! fixture through real `lir::lower_to_program`, proving that once a
//! typed/constructible map reaches this surface (B5, #1103), `for k, v in
//! m` lowers to exactly the F10-ruled desugar shape, unchanged from what
//! this test already pins today.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Issue #801: this crate's `clippy.toml` disallows bare `HashMap`/`HashSet`
// so an iteration-order leak into codegen output can't ship quietly. This
// file's one use (an always-empty `file_paths` map handed to
// `lir::lower_to_program` — this harness never populates `SourceLocation`)
// has no order to leak, matching `tests/lir_lowering.rs`'s own precedent
// for the identical pattern.
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map, no order to leak — see file doc"
)]

use brink_ir::lir;
use brink_ir::{DiagnosticCode, FileId, HirFile, SymbolManifest};

/// Lowers native source through the real production pipeline, stopping
/// short of codegen. `expect_e065_for` names exactly which identifiers are
/// expected to hit `E065` ("escapes strict inference as Unknown") — see
/// the module doc: native has no way to give a `for k, v in m` iterable a
/// map type today, so `k`/`v`/`m` unavoidably escape as `Unknown` under
/// native's unconditional strict typing. Any diagnostic for a *different*
/// identifier, or of a different code, still fails the fixture — this
/// isn't a blanket diagnostic suppression.
fn lower_native_program(src: &str, expect_e065_for: &[&str]) -> lir::Program {
    let parsed = brink_syntax_native::parse(src);
    assert!(
        parsed.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let file_id = FileId(0);
    let (hir, manifest, lower_diags) = brink_ir::hir::lower_native::lower(file_id, &parsed.tree());
    assert!(
        lower_diags.is_empty(),
        "unexpected native lowering diagnostics: {lower_diags:?}"
    );

    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, &hir, &manifest)];
    let analysis_opts = brink_analyzer::AnalysisOptions {
        dialect: brink_analyzer::Dialect::Brink,
        ..Default::default()
    };
    let analysis = brink_analyzer::analyze_with_options(&files_for_analysis, &analysis_opts);
    for d in &analysis.diagnostics {
        assert_eq!(
            d.code,
            DiagnosticCode::E065,
            "unexpected non-E065 diagnostic: {d:?}"
        );
        assert!(
            expect_e065_for.iter().any(|name| d.message.contains(name)),
            "unexpected E065 target (want one of {expect_e065_for:?}): {}",
            d.message
        );
    }
    assert_eq!(
        analysis.diagnostics.len(),
        expect_e065_for.len(),
        "expected exactly one E065 per {expect_e065_for:?}, got: {:?}",
        analysis.diagnostics
    );

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, &hir)];
    let (program, lir_diags) = lir::lower_to_program(
        &files_for_lir,
        &analysis.index,
        &analysis.resolutions,
        &std::collections::HashMap::new(),
    );
    assert!(
        lir_diags.is_empty(),
        "unexpected LIR lowering diagnostics: {lir_diags:?}"
    );
    program.expect("LIR lowering produced no program (see diagnostics)")
}

fn find_child<'a>(container: &'a lir::Container, name: &str) -> &'a lir::Container {
    container
        .children
        .iter()
        .find(|c| c.name.as_deref() == Some(name))
        .unwrap_or_else(|| {
            let names: Vec<Option<&str>> = container
                .children
                .iter()
                .map(|c| c.name.as_deref())
                .collect();
            panic!("no child named {name:?}, available: {names:?}")
        })
}

fn declare_temp_names<'a>(stmts: &[lir::Stmt], program: &'a lir::Program) -> Vec<&'a str> {
    stmts
        .iter()
        .filter_map(|s| match &s.kind {
            lir::StmtKind::DeclareTemp { name, .. } => {
                Some(program.name_table[name.0 as usize].as_str())
            }
            _ => None,
        })
        .collect()
}

/// `for k, v in m` (two-binding) desugars to: snapshot the container once
/// (`__for_container`, only because `v` needs to re-read it), snapshot its
/// keys (`__for_snapshot`), an index counter (`__for_idx`), then a
/// `LogicWhile` whose body declares `k` from the keys snapshot and `v` from
/// `__for_container[k]` — exactly the F10 "key-iteration + `let v = m[k]`"
/// desugar, before the loop's own body statements.
#[test]
fn two_binding_for_desugars_to_container_snapshot_plus_indexed_read() {
    let src = "\
fn test(m) {
  for k, v in m {
  }
}
";
    // `m` (untyped param), `k` and `v` (both bound `Unknown`, cascading
    // from `m`'s own `Unknown`) each get their own E065 — see the module
    // doc for why this is the honest, expected shape today.
    let program = lower_native_program(src, &["`m`", "`k`", "`v`"]);
    let test = find_child(&program.root, "test");

    let synth_names = declare_temp_names(&test.body, &program);
    assert_eq!(
        synth_names,
        vec!["__for_container", "__for_snapshot", "__for_idx"],
        "the two-binding form snapshots the container before the keys, \
         since `v` needs to index it again"
    );

    let lir::StmtKind::LogicWhile(logic_while) = &test
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::LogicWhile(_)))
        .expect("expected a LogicWhile")
        .kind
    else {
        unreachable!()
    };

    let body_names = declare_temp_names(&logic_while.body, &program);
    assert_eq!(
        body_names,
        vec!["k", "v"],
        "k declares first (from the keys snapshot), v second (from \
         `__for_container[k]`) — the F10-ruled `let v = m[k]` desugar"
    );

    // `v`'s value is `Index { base: GetTemp(__for_container), index: GetTemp(k) }`.
    // `lir::Stmt`/`lir::Expr` don't derive `Debug`, so failures below name
    // the expectation in prose rather than dumping the value.
    let lir::StmtKind::DeclareTemp { value: Some(v), .. } = &logic_while.body[1].kind else {
        panic!("expected v's DeclareTemp to hold a value");
    };
    let lir::ExprKind::Index { base, index } = &v.kind else {
        panic!("expected v's DeclareTemp to hold an Index expr");
    };
    let lir::ExprKind::GetTemp(_, base_name) = &base.kind else {
        panic!("expected v's Index base to read a temp");
    };
    assert_eq!(
        program.name_table[base_name.0 as usize], "__for_container",
        "v reads through the same snapshot the keys were taken from, not \
         the raw iterable expression (which must only evaluate once)"
    );
    let lir::ExprKind::GetTemp(_, index_name) = &index.kind else {
        panic!("expected v's Index index to read a temp");
    };
    assert_eq!(
        program.name_table[index_name.0 as usize], "k",
        "v is indexed by k, not by the raw loop counter"
    );
}

/// The pre-existing single-binding form is byte-for-byte unaffected: no
/// `__for_container` synthetic temp, no second `DeclareTemp` in the loop
/// body — this is the regression guard for the "single-binding form keeps
/// the original one-snapshot shape unchanged" claim in `blocks.rs`'s
/// `lower_for_stmt` doc.
#[test]
fn single_binding_for_is_unaffected_by_the_two_binding_desugar() {
    let src = "\
fn test(m) {
  for k in m {
  }
}
";
    let program = lower_native_program(src, &["`m`", "`k`"]);
    let test = find_child(&program.root, "test");

    let synth_names = declare_temp_names(&test.body, &program);
    assert_eq!(
        synth_names,
        vec!["__for_snapshot", "__for_idx"],
        "single-binding `for` never allocates a container snapshot temp"
    );

    let lir::StmtKind::LogicWhile(logic_while) = &test
        .body
        .iter()
        .find(|s| matches!(&s.kind, lir::StmtKind::LogicWhile(_)))
        .expect("expected a LogicWhile")
        .kind
    else {
        unreachable!()
    };
    let body_names = declare_temp_names(&logic_while.body, &program);
    assert_eq!(body_names, vec!["k"]);
}
