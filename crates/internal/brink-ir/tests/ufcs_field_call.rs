//! B3a UFCS `FieldCall` lowering (issue #1506): field access wins over a
//! free function of the same name, and the call lowers as a call
//! *through* the field's value (`lir::Expr::CallValue`) rather than the
//! desugared free call `name(recv, args)` `FreeFnDesugar` produces.
//!
//! The native surface cannot yet spell a function-typed struct field —
//! `struct` field types parse as bare paths
//! (`brink-syntax-native`'s `parser::decl::struct_field`) — so, mirroring
//! `brink-analyzer::tests::ufcs_resolution`'s own
//! `a_function_typed_field_wins_and_is_recorded_as_a_field_call`, this test
//! lowers ordinary source and then rewrites the one field's `TypeExpr` to
//! the function type the grammar will eventually admit. Everything
//! downstream of that rewrite — the analyzer's verdict, `brink-analyzer::
//! ufcs_lir_lookup`'s translation, and this crate's own LIR lowering — runs
//! for real; there is no fixture for a real `.brink` source reaching this
//! path today (see `brink-test-harness/tests/b3a_ufcs_e2e.rs`'s module doc).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Issue #801: this crate's `clippy.toml` disallows bare `HashMap`/`HashSet`
// so an iteration-order leak into codegen output can't ship quietly. This
// file's one use (an always-empty `file_paths` map handed to
// `lower_to_program_with_type_mode` — this test never populates
// `SourceLocation`) has no order to leak, the same exemption
// `tests/lir_lowering.rs` takes for the identical use.
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map, no order to leak — see file doc"
)]

use brink_ir::hir::lower_native;
use brink_ir::{FileId, TypeExpr, lir};
use rowan::{TextRange, TextSize};

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one straight-line pipeline walkthrough (parse -> analyze -> \
              infer -> resolve UFCS -> lower -> inspect); splitting it would \
              scatter the single fixture across several functions for no \
              reader benefit"
)]
fn a_field_call_verdict_lowers_as_a_call_through_the_fields_value() {
    let src = "\
struct Guest {
  greet: string
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
";
    let parsed = brink_syntax_native::parse(src);
    assert!(
        parsed.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parsed.errors()
    );
    let file_id = FileId(0);
    let (mut hir, manifest, lower_diags) = lower_native::lower(file_id, &parsed.tree());
    assert!(
        lower_diags.is_empty(),
        "lowering diagnostics: {lower_diags:?}"
    );

    // The one native-surface gap this test works around — see the module
    // doc. `greet`'s declared type becomes `fn(int): int`.
    let zero = TextRange::new(TextSize::from(0), TextSize::from(0));
    hir.structs[0].fields[0].ty = TypeExpr::Fn {
        params: vec![TypeExpr::Named {
            name: "int".into(),
            range: zero,
        }],
        ret: Box::new(TypeExpr::Named {
            name: "int".into(),
            range: zero,
        }),
        range: zero,
    };

    // The same hand-assembled "honest minimal native pipeline"
    // `brink-test-harness::corpus::compile_and_explore_from_brink_native`
    // runs — `brink_analyzer::analyze`'s pure path always passes
    // `is_native = false` internally (see that function's own doc), which
    // would misclassify this fixture as ink-syntax-under-strict-dialect and
    // reject the `STRUCT`/construction-literal brink extensions with E051.
    let (index, mut diagnostics) = brink_analyzer::symbol_index(&[(file_id, &manifest)]);
    let scope =
        brink_analyzer::ImportScope::new(hir.module.as_ref().map(|m| m.name.clone()), &hir.imports);
    let (file_resolutions, resolve_diags) =
        brink_analyzer::resolve(file_id, &manifest, &index, &scope);
    diagnostics.extend(resolve_diags);
    let mut resolutions = brink_analyzer::ResolutionMap::new();
    resolutions.extend(std::sync::Arc::unwrap_or_clone(file_resolutions));
    diagnostics.extend(brink_analyzer::per_file_diagnostics(
        file_id,
        &hir,
        &resolutions,
        &index,
        brink_analyzer::Dialect::StrictInk,
        true,
        None,
    ));

    let hir_inputs = vec![(file_id, &hir)];
    let manifest_inputs = vec![(file_id, &manifest)];
    let inline_docs = brink_analyzer::project_inline_docs(&manifest_inputs);
    let inference =
        brink_analyzer::infer_project(&hir_inputs, &index, &resolutions, None, &inline_docs);

    let files_for_meta = vec![(file_id, &hir, &manifest)];
    let (whole_diagnostics, _symbol_meta) = brink_analyzer::whole_project_diagnostics(
        &files_for_meta,
        &index,
        &resolutions,
        &brink_analyzer::AnalysisOptions::default(),
        Some(&inference),
    );
    diagnostics.extend(whole_diagnostics);
    assert!(
        diagnostics.is_empty(),
        "analysis diagnostics: {diagnostics:?}"
    );

    let (table, ufcs_diags) =
        brink_analyzer::ufcs_resolution(&hir_inputs, &index, &resolutions, &inference);
    assert!(ufcs_diags.is_empty(), "UFCS diagnostics: {ufcs_diags:?}");
    assert_eq!(table.len(), 1, "one UFCS site: {table:?}");
    let ufcs = brink_analyzer::ufcs_lir_lookup(&table);

    let (program, lir_diags) = lir::lower_to_program_with_type_mode(
        &hir_inputs,
        &index,
        &resolutions,
        &std::collections::HashMap::new(),
        lir::TypeMode::Gradual,
        &ufcs,
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

    // `lir::Stmt`/`Expr` derive neither `Debug` nor `PartialEq` (they hold
    // `Box<Expr>`/container trees with no need for either outside this kind
    // of structural check), so this walks and matches by hand rather than
    // via `assert_eq!`.
    let call_value = main_knot.body.iter().find_map(|stmt| match stmt {
        lir::Stmt::DeclareTemp {
            value: Some(lir::Expr::CallValue { callee, args }),
            ..
        } => Some((callee.as_ref(), args)),
        _ => None,
    });
    let Some((callee, args)) = call_value else {
        panic!(
            "expected a `DeclareTemp` initialized by `CallValue` in `main`'s {} body \
             statement(s), found none",
            main_knot.body.len()
        );
    };
    assert!(
        matches!(callee, lir::Expr::RecordGet { .. }),
        "the callee must read the field's value through a RecordGet chain"
    );
    assert_eq!(args.len(), 1, "the one call argument (`3`)");
    assert!(
        matches!(args[0], lir::Expr::Int(3)),
        "the sole call argument must be the literal `3`"
    );
}
