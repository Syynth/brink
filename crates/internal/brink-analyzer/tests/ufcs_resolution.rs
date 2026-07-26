//! B3a — UFCS resolution (issue #1482; D1–D5 RULED 2026-07-26,
//! `docs/decision-log.md` "UFCS resolution pass designed").
//!
//! Every fixture here is a real `.brink` file driven through the same seams
//! a user's compile drives: `brink_analyzer::analyze` for the index +
//! resolutions, `whole_project_diagnostics` for the errors, and
//! `ufcs_resolution` for the D2 verdict side table LIR lowering and IDE
//! hover read.
//!
//! One deliberate exception is documented at
//! [`a_function_typed_field_wins_and_is_recorded_as_a_field_call`]: a
//! *function-typed* struct field cannot be spelled on the native surface
//! yet (native `struct` field types are bare paths — there is no `fn(…): T`
//! type-expression grammar there), so that one fixture patches the lowered
//! `TypeExpr` directly. Its negative twin (`E140`, D1's hard error) needs no
//! such help and runs on plain source.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, UfcsVerdict};
use brink_ir::hir::lower_native;
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, HirFile, Name, SymbolManifest, TypeExpr, hir::visit,
};
use rowan::{TextRange, TextSize};

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

/// Whole-project diagnostics for one `.brink` file — the path a user's
/// compile takes (`brink-db`'s `whole_project_diagnostics_query` wraps this
/// exact function).
fn diagnostics(hir: &HirFile, manifest: &SymbolManifest) -> Vec<Diagnostic> {
    let files = vec![(FileId(0), hir, manifest)];
    let analysis = brink_analyzer::analyze(&files);
    let (diags, _meta) = brink_analyzer::whole_project_diagnostics(
        &files,
        &analysis.index,
        &analysis.resolutions,
        &AnalysisOptions::default(),
        None,
    );
    diags
}

/// The D2 verdict side table for one `.brink` file.
fn verdicts(hir: &HirFile, manifest: &SymbolManifest) -> Vec<UfcsVerdict> {
    let files = vec![(FileId(0), hir, manifest)];
    let analysis = brink_analyzer::analyze(&files);
    let hir_inputs = vec![(FileId(0), hir)];
    let manifest_inputs = vec![(FileId(0), manifest)];
    let inline_docs = brink_analyzer::project_inline_docs(&manifest_inputs);
    let inference = brink_analyzer::infer_project(
        &hir_inputs,
        &analysis.index,
        &analysis.resolutions,
        None,
        &inline_docs,
    );
    let (table, _diags) = brink_analyzer::ufcs_resolution(
        &hir_inputs,
        &analysis.index,
        &analysis.resolutions,
        &inference,
    );
    table.iter().map(|(_key, v)| v.clone()).collect()
}

fn codes(diags: &[Diagnostic]) -> Vec<DiagnosticCode> {
    diags.iter().map(|d| d.code).collect()
}

fn only(diags: &[Diagnostic], code: DiagnosticCode) -> &Diagnostic {
    let hits: Vec<&Diagnostic> = diags.iter().filter(|d| d.code == code).collect();
    assert_eq!(hits.len(), 1, "expected exactly one {code:?}: {diags:?}");
    hits[0]
}

// ─── Step 3: the free-function desugar ───────────────────────────────

const FREE_FN: &str = "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.greet(3);
}
";

#[test]
fn a_free_function_in_scope_resolves_and_is_recorded_as_a_desugar() {
    let (hir, manifest) = lower(FREE_FN);
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FreeFnDesugar { receiver, name, .. } = &verdicts[0] else {
        panic!("expected a free-fn desugar verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(name, "greet");
    assert_eq!(*receiver, brink_analyzer::Ty::Struct("Guest".into()));
}

/// The resolution replaces the pre-#1482 behavior outright: a UFCS-shaped
/// callee used to fall off the end of `resolve::resolve_function` as an
/// unresolved reference (`E025`). A *resolvable* method call must now be
/// completely diagnostic-free **at the analysis layer**.
///
/// It is still refused one layer down, at LIR lowering (`E144`), because
/// nothing consumes the D2 verdict side table yet — see
/// `brink-test-harness/tests/b3a_ufcs_e2e.rs` for that end of the path.
#[test]
fn a_resolved_method_call_raises_no_diagnostic_at_all() {
    let (hir, manifest) = lower(FREE_FN);
    let diags = diagnostics(&hir, &manifest);
    let relevant: Vec<&Diagnostic> = diags
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                DiagnosticCode::E025
                    | DiagnosticCode::E140
                    | DiagnosticCode::E141
                    | DiagnosticCode::E142
                    | DiagnosticCode::E143
            )
        })
        .collect();
    assert!(relevant.is_empty(), "expected none, got {relevant:?}");
}

// ─── Step 2 / D1: field access wins outright ─────────────────────────

/// D1: the receiver's type declares the called name as a field, but the
/// field is not callable. Field access **wins outright** — this is a hard
/// error, never a fall-through to the free `greet` that is also in scope.
#[test]
fn a_matching_non_callable_field_is_a_hard_error_and_never_falls_through() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  greet: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
",
    );

    // The verdict table records nothing — a hard error is not a resolution.
    assert!(verdicts(&hir, &manifest).is_empty());

    let diags = diagnostics(&hir, &manifest);
    let e140 = only(&diags, DiagnosticCode::E140);
    assert!(
        e140.message.contains("greet") && e140.message.contains("Guest"),
        "E140 must name the field and its shape: {}",
        e140.message
    );
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E141),
        "the free `greet` must not be reported as a second attempt: {diags:?}"
    );
}

/// The positive half of field-access-wins: a **function-typed** field is
/// called through its own value, and the verdict says so.
///
/// The native surface cannot spell a `fn(…): T` field type yet — `struct`
/// field types parse as bare paths (`brink-syntax-native`'s
/// `parser::decl::struct_field`), so this test lowers the ordinary source
/// and then rewrites the one field's `TypeExpr` to the function type the
/// grammar will eventually admit. Everything downstream of the shape table
/// (`structs::declared_shapes` → `ufcs::resolve`) is exercised for real.
#[test]
fn a_function_typed_field_wins_and_is_recorded_as_a_field_call() {
    let (mut hir, manifest) = lower(
        "\
struct Guest {
  greet: string
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
",
    );
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

    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FieldCall {
        receiver,
        field,
        field_ty,
    } = &verdicts[0]
    else {
        panic!("expected a field-call verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(*receiver, brink_analyzer::Ty::Struct("Guest".into()));
    assert_eq!(field, "greet");
    assert!(matches!(field_ty, brink_analyzer::Ty::Fn(..)));

    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E140),
        "a callable field is not an E140: {diags:?}"
    );
}

// ─── Step 4: neither attempt succeeded ───────────────────────────────

#[test]
fn neither_a_field_nor_a_free_function_is_one_diagnostic_naming_both() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.nope(3);
}
",
    );
    assert!(verdicts(&hir, &manifest).is_empty());
    let diags = diagnostics(&hir, &manifest);
    let e141 = only(&diags, DiagnosticCode::E141);
    assert!(
        e141.message.contains("no field `nope`"),
        "must name the field attempt: {}",
        e141.message
    );
    assert!(
        e141.message.contains("no function `nope` is in scope"),
        "must name the free-function attempt: {}",
        e141.message
    );
    assert!(
        e141.message.contains("Guest"),
        "must name the receiver's type: {}",
        e141.message
    );
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E025),
        "the pre-#1482 unresolved-reference error is replaced, not doubled: {diags:?}"
    );
}

// ─── D3: unknown receiver type demands an annotation ─────────────────

/// A parameter has no type on the native surface, so `g`'s type is
/// genuinely unknown at the resolution point. D3 rules that an error, not a
/// deferral — and the error must say so rather than blaming the method.
#[test]
fn an_unknown_receiver_type_demands_an_annotation() {
    let (hir, manifest) = lower(
        "\
fn greet(g, loudness) {
  return loudness;
}

fn main(guest) {
  let n = guest.greet(3);
}
",
    );
    assert!(verdicts(&hir, &manifest).is_empty());
    let diags = diagnostics(&hir, &manifest);
    let e142 = only(&diags, DiagnosticCode::E142);
    assert!(
        e142.message.contains("annotate"),
        "E142 must demand an annotation: {}",
        e142.message
    );
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E141),
        "an unknown receiver is not a both-attempts-failed site: {diags:?}"
    );
}

// ─── D5 fence: auto-ref is #1462, not this pass ──────────────────────

#[test]
fn a_ref_first_param_free_fn_is_refused_with_a_pointer_to_1462() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn heal(ref g, amount) {
  return amount;
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.heal(5);
}
",
    );
    assert!(
        verdicts(&hir, &manifest).is_empty(),
        "a `ref` first param must never be desugared by value"
    );
    let diags = diagnostics(&hir, &manifest);
    let e143 = only(&diags, DiagnosticCode::E143);
    assert!(
        e143.message.contains("#1462"),
        "E143 must point at the follow-up: {}",
        e143.message
    );
}

// ─── Scope fences ────────────────────────────────────────────────────

/// Each call in a chain resolves independently — the pass keys verdicts by
/// call-site range, never by chain. Here the inner call resolves (free fn)
/// and the outer one fails, and the two verdicts do not interfere.
#[test]
fn each_call_in_a_chain_resolves_independently() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let g = Guest { name: \"ada\" };
  let a = g.greet(1);
  let b = g.nope(2);
}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "only the resolvable call: {verdicts:?}");
    assert!(matches!(verdicts[0], UfcsVerdict::FreeFnDesugar { .. }));
    let diags = diagnostics(&hir, &manifest);
    only(&diags, DiagnosticCode::E141);
}

/// A bare `a.b` field read is untouched: only the final pre-`(` segment of
/// a *call* gets UFCS treatment.
#[test]
fn a_bare_dotted_read_is_not_a_ufcs_site() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.name;
}
",
    );
    assert!(verdicts(&hir, &manifest).is_empty());
    let diags = diagnostics(&hir, &manifest);
    for code in [
        DiagnosticCode::E140,
        DiagnosticCode::E141,
        DiagnosticCode::E142,
        DiagnosticCode::E143,
    ] {
        assert!(
            !codes(&diags).contains(&code),
            "{code:?} must not fire on a bare field read: {diags:?}"
        );
    }
}

/// The explicit free-call spelling is unaffected — a bare `greet(g, 3)`
/// never enters the pass.
#[test]
fn the_explicit_free_call_spelling_is_untouched() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = greet(g, 3);
}
",
    );
    assert!(verdicts(&hir, &manifest).is_empty());
}

/// The ink dialect is untouched **by construction**: ink's own
/// `FunctionCall` lowering always builds a single-segment callee path, so
/// no ink source can produce the shape this pass keys on. Pinned here so a
/// future change to ink's lowering that broke that invariant fails loudly
/// rather than silently dragging the oracle corpus into a type-directed
/// resolution pass.
#[test]
fn ink_never_produces_a_multi_segment_callee_path() {
    let parse = brink_syntax::parse(
        "== start ==\n~ temp v = f(1)\n~ temp w = knot_a.stitch_b\n-> END\n\n== knot_a ==\n= stitch_b\n-> END\n",
    );
    assert!(parse.errors().is_empty(), "{:?}", parse.errors());
    let (hir, _manifest, _diags) = brink_ir::hir::lower(FileId(0), &parse.tree());

    struct Scan {
        multi: Vec<String>,
    }
    impl visit::HirVisitor for Scan {
        fn visit_exprs(&self) -> bool {
            true
        }
        fn enter_expr(&mut self, expr: &brink_ir::Expr) {
            if let brink_ir::Expr::Call(path, _) = expr
                && path.segments.len() > 1
            {
                self.multi.push(
                    path.segments
                        .iter()
                        .map(|s: &Name| s.text.clone())
                        .collect::<Vec<_>>()
                        .join("."),
                );
            }
        }
    }
    let mut scan = Scan { multi: Vec::new() };
    visit::visit(&hir, &mut scan);
    assert!(
        scan.multi.is_empty(),
        "ink lowering produced a multi-segment callee path: {:?}",
        scan.multi
    );
}
