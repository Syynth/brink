//! B3a — UFCS resolution (issue #1482; D1–D5 RULED 2026-07-26,
//! `docs/decision-log.md` "UFCS resolution pass designed").
//!
//! Every fixture here is a real `.brink` file driven through the same seams
//! a user's compile drives: `brink_analyzer::analyze` for the index +
//! resolutions, `whole_project_diagnostics` for the errors, and
//! `ufcs_resolution` for the D2 verdict side table LIR lowering and IDE
//! hover read.
//!
//! Every declared field *type* is real source too, including the
//! function-typed and generic (`Array<int>`) shapes NG-E (issue #1505)
//! unblocked by widening `brink-syntax-native`'s `struct_field` grammar from
//! a bare `PATH` to the real `type_expr` production. The one remaining
//! device is [`array_receiver_fixture`]'s field *value* (`items: 0` for a
//! declared `Array<int>`) — the native surface still has no array-literal
//! grammar to construct a real value of that type with, so only the
//! receiver's declared type is trustworthy there, never its value.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_analyzer::{AnalysisOptions, ModuleMap, ResolvedModule, UfcsVerdict};
use brink_ir::hir::lower_native;
use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile, Name, SymbolManifest, hir::visit};

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
        // `is_native`: an ink fixture (issue #1358).
        false,
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
/// LIR lowering now consumes this verdict for real (issue #1506) instead of
/// refusing every resolved site with `E144` — see
/// `brink-test-harness/tests/b3a_ufcs_e2e.rs` for that end of the path (the
/// e2e proof that the call actually compiles and plays).
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

/// A resolved free-fn desugar owes the same arity check every other
/// resolved call gets (`resolve::check_arity`) — the receiver counts as the
/// first argument, so `g.greet(1, 2, 3)` against `fn greet(g, loudness)`
/// (two params) is a 4-vs-2 mismatch, not a clean resolution.
#[test]
fn a_free_fn_desugar_with_the_wrong_arity_is_still_reported() {
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
  let n = g.greet(1, 2, 3);
}
",
    );
    // The site still resolves — an arity mismatch is a diagnostic
    // alongside the verdict, not a refusal to record one (mirrors
    // `resolve::check_arity`'s own behavior for every other call).
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    assert!(matches!(verdicts[0], UfcsVerdict::FreeFnDesugar { .. }));

    let diags = diagnostics(&hir, &manifest);
    let e031 = only(&diags, DiagnosticCode::E031);
    assert!(
        e031.message.contains("greet"),
        "E031 must name the function: {}",
        e031.message
    );
}

// ─── Step 3: the T1b/NS stdlib prelude fallback (D4) ──────────────────

/// A receiver typed `Array<int>` — NG-E (issue #1505) means this is now
/// spelled directly (`items: Array<int>`), no more HIR-patching the
/// `TypeExpr` after the fact. The field's *value* (`items: 0`) still doesn't
/// match its declared type — the native surface has no array-literal
/// grammar yet to construct a real `Array<int>` *value* with — but that's
/// harmless here: the field's declared type (read straight from
/// `structs::declared_shapes`, not from inference) is all a receiver's type
/// needs, and the shape backing it is otherwise inert.
fn array_receiver_fixture(call_expr: &str) -> (HirFile, SymbolManifest) {
    lower(&format!(
        "\
struct Bag {{
  items: Array<int>
}}

fn main() {{
  let b = Bag {{ items: 0 }};
  let n = {call_expr};
}}
"
    ))
}

/// D4's candidate set for step 3 is "ordinary lexical scope only (file
/// `use` + prelude)": the T1b/NS stdlib prelude (`len`, `push`, `sort_by`,
/// …) is not an index symbol, so `resolve::lookup_by_name`'s `Knot`/
/// `External` sweep alone would miss it and a legal `b.items.len()` would
/// read as "no function `len` is in scope here" (`E141`) — which is false,
/// `len(xs)` compiles today.
#[test]
fn a_prelude_verb_resolves_and_is_recorded_as_a_prelude_desugar() {
    let (hir, manifest) = array_receiver_fixture("b.items.len()");
    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E141),
        "a prelude verb must not read as \"no function in scope\": {diags:?}"
    );

    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::PreludeDesugar { receiver, name, .. } = &verdicts[0] else {
        panic!("expected a prelude-desugar verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(name, "len");
    assert_eq!(
        *receiver,
        brink_analyzer::Ty::Array(Box::new(brink_analyzer::Ty::Int))
    );
}

/// A mutating prelude verb (`push`) resolves the same way — arrays have no
/// declared shape, so the field-wins step (2) never intercepts it.
#[test]
fn a_mutating_prelude_verb_also_resolves_as_a_prelude_desugar() {
    let (hir, manifest) = array_receiver_fixture("b.items.push(3)");
    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E141),
        "a prelude verb must not read as \"no function in scope\": {diags:?}"
    );

    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::PreludeDesugar { name, .. } = &verdicts[0] else {
        panic!("expected a prelude-desugar verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(name, "push");
}

// ─── Receiver typed from the resolved definition, not file-locally ────

/// The resolver (`resolve::resolve_function`'s UFCS-shaped fallback) binds
/// a call's head to a value *project-wide* (`resolve::lookup_by_name`), not
/// file-scoped. The receiver must be typed from that same resolved
/// definition — a global `VAR` declared in another file must type
/// correctly here, not read as an unknown receiver demanding an annotation
/// (`E142`) just because the naive "look it up by name in this file" path
/// can't see it. (`int`, not a struct: `int` isolates the file-scoping bug
/// this test pins from the typing question. A struct-typed global *does*
/// reach `infer::collect_globals`'s map since issue #1540 gave `Sig` a
/// full-fidelity `value_ty`; before that it could not, whatever this pass
/// did.)
#[test]
fn a_multi_file_global_receiver_is_typed_from_the_resolved_definition() {
    let (hir_a, manifest_a) = lower("var g: int = 10\n");
    let (hir_b, manifest_b) = lower(
        "\
fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let n = g.greet(3);
}
",
    );

    let files = vec![
        (FileId(0), &hir_a, &manifest_a),
        (FileId(1), &hir_b, &manifest_b),
    ];
    let analysis = brink_analyzer::analyze(&files);
    let (diags, _meta) = brink_analyzer::whole_project_diagnostics(
        &files,
        &analysis.index,
        &analysis.resolutions,
        &AnalysisOptions::default(),
        // `is_native`: an ink fixture (issue #1358).
        false,
        None,
    );
    assert!(
        !diags.iter().any(|d| d.code == DiagnosticCode::E142),
        "a global receiver declared in another file must not read as unknown: {diags:?}"
    );

    let hir_inputs = vec![(FileId(0), &hir_a), (FileId(1), &hir_b)];
    let manifest_inputs = vec![(FileId(0), &manifest_a), (FileId(1), &manifest_b)];
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
    let verdicts: Vec<_> = table.iter().map(|(_key, v)| v.clone()).collect();
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FreeFnDesugar { receiver, name, .. } = &verdicts[0] else {
        panic!("expected a free-fn desugar verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(name, "greet");
    assert_eq!(*receiver, brink_analyzer::Ty::Int);
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
/// NG-E (issue #1505) widened `brink-syntax-native`'s `struct_field`
/// grammar from a bare `PATH` to the real `type_expr` production, so a
/// `fn(…): T` field type is spelled directly on real source — no more
/// rewriting the lowered `TypeExpr` by hand. Everything from parsing
/// onward — the shape table (`structs::declared_shapes` → `ufcs::resolve`)
/// included — is exercised for real.
#[test]
fn a_function_typed_field_wins_and_is_recorded_as_a_field_call() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  greet: fn(int): int
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
",
    );

    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FieldCall {
        receiver,
        field,
        field_ty,
        arity_mismatch,
        arg_mismatches,
    } = &verdicts[0]
    else {
        panic!("expected a field-call verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(*receiver, brink_analyzer::Ty::Struct("Guest".into()));
    assert_eq!(field, "greet");
    assert!(matches!(field_ty, brink_analyzer::Ty::Fn(..)));
    // Issue #1918: `g.greet(3)` matches `greet: fn(int): int`'s one
    // parameter both in arity and type — a correctly-typed field call must
    // stay clean, not just resolve.
    assert_eq!(*arity_mismatch, None, "correct arity must stay clean");
    assert!(
        arg_mismatches.is_empty(),
        "correct argument types must stay clean: {arg_mismatches:?}"
    );

    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E140),
        "a callable field is not an E140: {diags:?}"
    );
}

/// Review finding on issue #1909: `infer_ufcs_free_fn_result` records its
/// call-graph edge ([`Self::record_call_edge`] in `infer/body.rs`) *before*
/// the struct-receiver gate that declines a field-call site's result type —
/// see that function's own "Why the edge is recorded before the receiver is
/// even typed" doc. This pins the consequence directly: a struct receiver
/// whose shape declares an `Fn`-typed field of the called name (so D1 says
/// field access wins outright, and [`UfcsVerdict::FieldCall`] is the real
/// verdict) still records a call-graph edge to a same-named, matching-arity
/// free function that is never actually invoked. That is a deliberate,
/// safe-direction over-approximation — the recorded call graph is a
/// *superset* of the real one, never a subset — and this test pins both
/// halves: the result type stays `Unknown` (the free function's return type
/// never leaks into the field-call site), and the edge is recorded anyway.
#[test]
fn a_shadowed_free_function_still_gets_a_call_edge_despite_the_field_winning() {
    let source = "\
struct Guest {
  greet: fn(int): int
}

fn greet(g, loudness) {
  return loudness;
}

fn caller() {
  let g = Guest { greet: \"hi\" };
  return g.greet(3);
}
";
    let (hir, manifest) = lower(source);

    // The verdict is FieldCall, not a free-fn desugar — D1 field access wins.
    let vs = verdicts(&hir, &manifest);
    assert_eq!(vs.len(), 1, "one UFCS site: {vs:?}");
    assert!(
        matches!(vs[0], UfcsVerdict::FieldCall { .. }),
        "field access must win over the same-named free function: {:?}",
        vs[0]
    );

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

    let caller_id = analysis
        .index
        .by_name
        .get("caller")
        .and_then(|ids| ids.first())
        .copied()
        .expect("caller");
    let greet_fn_id = analysis
        .index
        .by_name
        .get("greet")
        .and_then(|ids| ids.first())
        .copied()
        .expect("free fn greet");

    // The result type does NOT leak the free function's return type: the
    // struct-receiver gate declines, so `caller`'s inferred return stays
    // Unknown, never `int` (`greet`'s declared return type).
    let caller_body = inference.bodies.get(&caller_id).expect("caller body");
    assert_eq!(
        caller_body.return_ty,
        brink_analyzer::Ty::Unknown,
        "the struct-receiver gate must decline the result type"
    );

    // The call edge IS recorded anyway — the documented over-approximation.
    let inferable = brink_analyzer::inferable_defs_from_index(&analysis.index);
    let edges = brink_analyzer::call_edges(
        caller_id,
        &hir_inputs,
        &analysis.index,
        &analysis.resolutions,
        &inferable,
        None,
    );
    assert!(
        edges.contains(&greet_fn_id),
        "the edge to the shadowed free function must still be recorded: {edges:?}"
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

// ─── issue #2793: the ordinary (non-lambda) fn/knot annotated-param half
// of #2786's `BodyTypes::locals` visibility fix ───────────────────────

/// #2786 overlaid an *ordinary* `fn`/knot param's own written annotation
/// onto `pass.locals` whenever the body walk left it absent — the same
/// mechanism `option_conditions.rs`'s
/// `annotated_fn_param_option_condition_is_e116` pins for E116. Here it is
/// this file's own D3 check (`ufcs.rs::UfcsVisitor::head_ty`, which reads
/// `current_locals()` — the same `BodyTypes::locals` — for a `Param`/`Temp`
/// receiver) that benefits: `g`'s only appearance in `main`'s body is as
/// the UFCS receiver itself, so no other statement observes it (mirrors
/// `an_unknown_receiver_type_demands_an_annotation` just above, minus the
/// annotation — that fixture's own `guest` param is genuinely unannotated).
/// Pre-#2786, `g` stayed absent from `pass.locals`, `receiver_ty` returned
/// `None`, and D3 demanded an annotation (`E142`) even though one was
/// already written. Post-#2786 the annotation itself supplies the
/// classification, so the receiver resolves and the call desugars — the new
/// true positive #2793 asks each consumer to confirm (inverted from a
/// typical "new diagnostic": here the fix's effect is a diagnostic that no
/// longer fires on legal, already-annotated source).
#[test]
fn an_annotated_fn_param_receiver_resolves_from_its_own_annotation() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main(g: Guest) {
  let n = g.greet(3);
}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FreeFnDesugar { receiver, name, .. } = &verdicts[0] else {
        panic!("expected a free-fn desugar verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(name, "greet");
    assert_eq!(*receiver, brink_analyzer::Ty::Struct("Guest".into()));

    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E142),
        "the ordinary fn param's own `: Guest` annotation must resolve the \
         receiver without demanding a further one: {diags:?}"
    );
}

// ─── issue #2773: a lambda-own receiver must not inherit an outer
// same-named binding's type ──────────────────────────────────────────

/// Review finding on issue #2773: everywhere else in that fix,
/// "unclassifiable" means *silence* — but here it means a **new hard
/// error on source that previously compiled**, so it must be pinned
/// deliberately rather than left to fall out.
///
/// `pruned_locals_for_lambda` prunes every lambda param name from the
/// inherited frame and re-seeds only the ones carrying a resolvable `: T`
/// annotation. The inner `s` has none, so `head_ty` finds nothing,
/// `receiver_ty` returns `None`, and `resolve_call` pushes `E142`.
///
/// Pre-fix this fixture resolved the inner `s` to the *enclosing* `f`'s
/// same-named param by bare name — a binding that is not the one in scope.
/// The new error is the correct direction (it makes the shadowing case
/// agree with `an_unknown_receiver_type_demands_an_annotation` above, where
/// a plain unannotated param already earned `E142`), but it is a behavior
/// change for existing sources and is recorded as such in the changeset.
#[test]
fn an_unannotated_shadowing_lambda_param_receiver_demands_an_annotation() {
    let (hir, manifest) = lower(
        "\
fn shout(s: string, times: int) {
  return times;
}

fn f(s: string) {
  let g = |s| s.shout(2);
}
",
    );
    let diags = diagnostics(&hir, &manifest);
    let e142 = only(&diags, DiagnosticCode::E142);
    assert!(
        e142.message.contains("annotate"),
        "E142 must demand an annotation: {}",
        e142.message
    );
}

/// The control half: the same shadowing shape, but the lambda's own param
/// carries its own `: string` annotation, so `pruned_locals_for_lambda`
/// seeds it back and the receiver resolves — from the lambda's own written
/// type, never the enclosing `f`'s same-named binding. No `E142`.
#[test]
fn an_annotated_shadowing_lambda_param_receiver_resolves_from_its_own_annotation() {
    let (hir, manifest) = lower(
        "\
fn shout(s: string, times: int) {
  return times;
}

fn f(s: int) {
  let g = |s: string| s.shout(2);
}
",
    );
    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E142),
        "the lambda's own `: string` annotation must resolve the receiver: {diags:?}"
    );
}

// ─── D5: auto-ref (issue #1462) ──────────────────────────────────────

/// A `ref` first parameter turns the desugar into the auto-ref shape — never
/// the by-value one, which would silently drop the mutation — and the site is
/// otherwise a clean resolution.
#[test]
fn a_ref_first_param_auto_refs_a_frame_local_receiver() {
    let (hir, manifest) = lower(
        "\
fn bump(ref n, amount) {
  n = n + amount;
}

fn main() {
  let g = 1;
  g.bump(5);
}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FreeFnAutoRef { receiver, name, .. } = &verdicts[0] else {
        panic!("expected an auto-ref verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(name, "bump");
    assert_eq!(*receiver, brink_analyzer::Ty::Int);
    assert!(
        !codes(&diagnostics(&hir, &manifest)).contains(&DiagnosticCode::E143),
        "a writable receiver is not refused"
    );
}

/// The durable half of the same rule: a global `VAR` receiver auto-refs too.
#[test]
fn a_ref_first_param_auto_refs_a_global_var_receiver() {
    let (hir, manifest) = lower(
        "\
var hp: int = 1

fn bump(ref n, amount) {
  n = n + amount;
}

fn main() {
  hp.bump(5);
}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    assert!(matches!(verdicts[0], UfcsVerdict::FreeFnAutoRef { .. }));
}

/// The auto-ref desugar owes the same arity check the by-value one does —
/// the receiver still counts as the first argument.
#[test]
fn an_auto_ref_desugar_with_the_wrong_arity_is_still_reported() {
    let (hir, manifest) = lower(
        "\
fn bump(ref n, amount) {
  n = n + amount;
}

fn main() {
  let g = 1;
  g.bump(1, 2, 3);
}
",
    );
    let diags = diagnostics(&hir, &manifest);
    let e031 = only(&diags, DiagnosticCode::E031);
    assert!(
        e031.message.contains("bump"),
        "E031 must name the function: {}",
        e031.message
    );
}

/// D5's refusal: an immutable receiver under a `ref` first parameter is a
/// compile error, never a by-value desugar that drops the mutation.
#[test]
fn a_const_receiver_under_a_ref_first_param_is_refused() {
    let (hir, manifest) = lower(
        "\
const START: int = 1

fn bump(ref n, amount) {
  n = n + amount;
}

fn main() {
  START.bump(5);
}
",
    );
    assert!(
        verdicts(&hir, &manifest).is_empty(),
        "an unwritable receiver must never be desugared"
    );
    let diags = diagnostics(&hir, &manifest);
    let e143 = only(&diags, DiagnosticCode::E143);
    assert!(
        e143.message.contains("cannot mutate") && e143.message.contains("CONST"),
        "E143 must name the cause: {}",
        e143.message
    );
}

/// **RULED 2026-07-27** (issue #1531, `docs/decision-log.md`): a projection
/// whose root is a frame-local is a legal `ref`-first-param receiver, one
/// field level deep — a frame-local cell is a valid projection root, and
/// the mutation needs no effect row (unobservable outside the frame). The
/// analyzer records the ordinary `FreeFnAutoRef` verdict; LIR lowering
/// (`brink_ir::lir::lower::blocks::try_lower_frame_local_auto_ref_stmt`) is
/// what supplies the actual (non-`RefProjection`) lowering for it.
#[test]
fn a_projection_off_a_frame_local_under_a_ref_first_param_is_accepted() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  hp: int
}

fn heal(ref h, amount) {
  h = h + amount;
}

fn main() {
  let g = Guest { hp: 1 };
  g.hp.heal(5);
}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    assert!(matches!(verdicts[0], UfcsVerdict::FreeFnAutoRef { .. }));
    assert!(
        !codes(&diagnostics(&hir, &manifest)).contains(&DiagnosticCode::E143),
        "a single-field-deep frame-local projection is a legal receiver"
    );
}

/// The ruling's own boundary: the analyzer gate only clears a frame-local
/// projection **one field level deep** — LIR's RMW expansion for it is
/// statement-shaped and single-level, the same boundary plain assignment
/// draws (`try_lower_field_assignment`'s `E074`). A deeper chain off a
/// frame-local still refuses.
#[test]
fn a_two_field_deep_projection_off_a_frame_local_under_a_ref_first_param_is_refused() {
    let (hir, manifest) = lower(
        "\
struct Hp {
  current: int
}

struct Guest {
  hp: Hp
}

fn heal(ref h, amount) {
  h = h + amount;
}

fn main() {
  let g = Guest { hp: Hp { current: 1 } };
  g.hp.current.heal(5);
}
",
    );
    assert!(verdicts(&hir, &manifest).is_empty());
    let diags = diagnostics(&hir, &manifest);
    let e143 = only(&diags, DiagnosticCode::E143);
    assert!(
        e143.message.contains("cannot mutate") && e143.message.contains("one field"),
        "E143 must name the field-depth rule: {}",
        e143.message
    );
}

/// The mirror of the rule: a **non-`ref`** first parameter puts no lvalue
/// requirement on the receiver at all — the very `const` receiver refused
/// above resolves cleanly as the ordinary by-value desugar.
#[test]
fn a_non_ref_first_param_accepts_an_immutable_receiver() {
    let (hir, manifest) = lower(
        "\
const START: int = 1

fn plus(n, amount) {
  return n + amount;
}

fn main() {
  let n = START.plus(5);
}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    assert!(matches!(verdicts[0], UfcsVerdict::FreeFnDesugar { .. }));
    assert!(
        !codes(&diagnostics(&hir, &manifest)).contains(&DiagnosticCode::E143),
        "the by-value desugar has no lvalue requirement"
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

/// Collects every multi-segment callee path an HIR file contains — the
/// shape `brink-analyzer::ufcs` keys on.
#[derive(Default)]
struct MultiSegmentCalleeScan {
    found: Vec<String>,
}

impl visit::HirVisitor for MultiSegmentCalleeScan {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &brink_ir::Expr) {
        if let brink_ir::Expr::Call(path, _) = expr
            && path.segments.len() > 1
        {
            self.found.push(
                path.segments
                    .iter()
                    .map(|s: &Name| s.text.clone())
                    .collect::<Vec<_>>()
                    .join("."),
            );
        }
    }
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

    let mut scan = MultiSegmentCalleeScan::default();
    visit::visit(&hir, &mut scan);
    assert!(
        scan.found.is_empty(),
        "ink lowering produced a multi-segment callee path: {:?}",
        scan.found
    );
}

/// A `#fn(target)` literal's target is *also* recorded as a
/// `RefKind::Function` reference, but it is not a call site and has no UFCS
/// verdict — so `resolve::resolve_function`'s UFCS-shaped fallback must not
/// claim it. The manifest's own `arg_count` distinction is what separates
/// them (`#fn` binds a prefix of the param row, so it records `None`), and
/// this pins that a dotted `#fn` target keeps failing as an unresolved
/// reference rather than silently resolving to its head value.
#[test]
fn a_dotted_fn_literal_target_is_not_claimed_as_a_ufcs_callee() {
    let parse = brink_syntax::parse("VAR g = 0\n\n== start ==\n~ temp f = #fn(g.greet)\n-> END\n");
    assert!(parse.errors().is_empty(), "{:?}", parse.errors());
    let (hir, manifest, _diags) = brink_ir::hir::lower(FileId(0), &parse.tree());
    let files = vec![(FileId(0), &hir, &manifest)];
    let analysis = brink_analyzer::analyze(&files);
    assert!(
        analysis
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E025),
        "expected the unresolved-reference error to survive: {:?}",
        analysis.diagnostics
    );
}

// ─── issue #2253 review finding F3: UFCS field-call resolution must use ───
// ─── the referrer's own shape under a std/project name collision ─────────

/// F3 (blocking, PR #2253 review): `ufcs::try_field_call`/`receiver_ty` walk
/// `ShapeTable::resolve`, the same lookup `structs`/`ref_projection` use —
/// but until this fix landed nothing in the suite exercised it under a
/// same-named struct declared by two coexisting modules (the stdlib mount,
/// #2080's M-2d scenario). This is that missing UFCS case.
///
/// Project and std each declare their own `Guest` shape, deliberately with
/// *incompatible* field types for the same field name: the project's own
/// `greet` is callable (`fn(int): int`), std's is not (`string`). If UFCS
/// field-call resolution ever picked std's `Guest` for a reference inside
/// the project file, `g.greet(3)` would hit the E140 "field exists but
/// isn't callable" hard error instead of resolving as a clean
/// `UfcsVerdict::FieldCall` — the same "wrong struct's fields" failure mode
/// #2241 describes, exercised through this consumer specifically (the one
/// F3 named as under-covered).
///
/// Rule 20a: verified this test FAILS (E140 fires, zero `FieldCall`
/// verdicts) against a `ShapeTable::resolve` that ignores its `scope`
/// argument and always prefers a std-declared candidate when one exists —
/// restored before committing.
#[test]
fn ufcs_field_call_resolves_the_referrers_own_shape_when_std_and_project_share_a_name() {
    let project_src = "\
struct Guest {
  greet: fn(int): int
}

fn main() {
  let g = Guest { greet: \"hi\" };
  let n = g.greet(3);
}
";
    let std_src = "\
struct Guest {
  greet: string
}

fn noop() {}
";

    let (project_hir, project_manifest) = lower(project_src);
    let (std_hir, std_manifest) = lower(std_src);
    let project_file = FileId(0);
    let std_file = FileId(1);

    let mut modules = ModuleMap::new();
    modules.insert(
        project_file,
        ResolvedModule {
            name: "story::main".to_string(),
            declared: true,
            was: None,
        },
    );
    modules.insert(
        std_file,
        ResolvedModule {
            name: "std::conventions::screenplay".to_string(),
            declared: true,
            was: None,
        },
    );

    let files = vec![
        (project_file, &project_hir, &project_manifest),
        (std_file, &std_hir, &std_manifest),
    ];
    let analysis = brink_analyzer::analyze_with_modules(
        &files,
        &modules,
        &AnalysisOptions::default(),
        // Native `.brink` fixtures throughout this file (issue #1358).
        true,
    );
    assert!(
        !analysis
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::E023),
        "cross-declared-module `Guest`s must coexist with no duplicate-declaration \
         diagnostic: {:?}",
        analysis.diagnostics
    );

    let hir_inputs = vec![(project_file, &project_hir), (std_file, &std_hir)];
    let manifest_inputs = vec![(project_file, &project_manifest), (std_file, &std_manifest)];
    let inline_docs = brink_analyzer::project_inline_docs(&manifest_inputs);
    let inference = brink_analyzer::infer_project(
        &hir_inputs,
        &analysis.index,
        &analysis.resolutions,
        None,
        &inline_docs,
    );
    let (table, ufcs_diags) = brink_analyzer::ufcs_resolution(
        &hir_inputs,
        &analysis.index,
        &analysis.resolutions,
        &inference,
    );

    assert!(
        !ufcs_diags.iter().any(|d| d.code == DiagnosticCode::E140),
        "the project's own callable `greet` field must resolve a clean field-call verdict, \
         not std's non-callable one: {ufcs_diags:?}"
    );
    let verdicts: Vec<_> = table.iter().map(|(_key, v)| v.clone()).collect();
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FieldCall {
        receiver,
        field,
        field_ty,
        ..
    } = &verdicts[0]
    else {
        panic!("expected a field-call verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(*receiver, brink_analyzer::Ty::Struct("Guest".into()));
    assert_eq!(field, "greet");
    assert!(
        matches!(field_ty, brink_analyzer::Ty::Fn(..)),
        "resolved against std's non-callable `greet: string` instead of the project's own \
         `greet: fn(int): int`: {field_ty:?}"
    );
}

// ─── issue #2096: a UFCS call inside a decl-default lambda's own body ────
// ─── (a file-level `VAR`/`CONST` initializer) ─────────────────────────

/// The laziness gate itself (`project_has_ufcs_call`) must see a
/// dotted-callee call sitting inside a `CONST` initializer's own lambda
/// body — not just the block tree — or `ufcs::resolve` (and
/// `check_strict`, and `assemble_analyzer_tables`'s `needs_ufcs`) would
/// never even be invoked for a project whose only UFCS-shaped call is one
/// of these.
#[test]
fn project_has_ufcs_call_sees_a_call_inside_a_decl_default_lambda_body() {
    let (hir, _manifest) = lower(
        "\
fn greet(g, loudness) {
  return loudness;
}

const callGreet = |g: int| g.greet(3)

fn main() {}
",
    );
    assert!(
        brink_analyzer::project_has_ufcs_call(&hir),
        "a UFCS-shaped call inside a CONST decl-default's own lambda body must trip the \
         laziness gate, or the whole pass never runs for this project"
    );
}

/// The pass itself must now visit that call site and record a real verdict
/// for it — before this fix, `ufcs::resolve` drove `UfcsVisitor` with plain
/// `visit::visit`, which never reaches a `VAR`/`CONST` initializer at all,
/// so this table was empty and the call fell through to LIR lowering's
/// defensive `E144` refusal instead. `Guest` declares no `greet` field, so
/// D1 loses and this resolves as an ordinary free-fn desugar — the same
/// verdict `a_free_function_in_scope_resolves_and_is_recorded_as_a_desugar`
/// pins for the non-lambda-decl-default shape.
#[test]
fn a_ufcs_call_inside_a_const_decl_default_lambda_body_resolves() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

const callGreet = |g: Guest| g.greet(3)

fn main() {}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FreeFnDesugar { receiver, name, .. } = &verdicts[0] else {
        panic!("expected a free-fn desugar verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(name, "greet");
    assert_eq!(*receiver, brink_analyzer::Ty::Struct("Guest".into()));

    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E144),
        "must never fall through to the old defensive never-visited refusal: {diags:?}"
    );
}

/// The `VAR` sibling of the test above — `ufcs::resolve` walks
/// `hir.variables` too, not only `hir.constants`.
#[test]
fn a_ufcs_call_inside_a_var_decl_default_lambda_body_resolves() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

var callGreet = |g: Guest| g.greet(3)

fn main() {}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    assert!(matches!(verdicts[0], UfcsVerdict::FreeFnDesugar { .. }));
}

/// Without a receiver-type annotation, this pass now correctly demands one
/// (D3, `E142`) instead of silently never seeing the call — the visitor is
/// reached (unlike before this fix), but the receiver's type is genuinely
/// undecidable from `|g|` alone with nothing else constraining it, exactly
/// like `an_unknown_receiver_type_demands_an_annotation` above (the
/// non-lambda-decl-default sibling of this same D3 rule).
#[test]
fn an_unannotated_decl_default_lambda_receiver_demands_an_annotation() {
    let (hir, manifest) = lower(
        "\
fn greet(g, loudness) {
  return loudness;
}

const callGreet = |g| g.greet(3)

fn main() {}
",
    );
    assert!(
        verdicts(&hir, &manifest).is_empty(),
        "an undecidable receiver must record no verdict"
    );
    let diags = diagnostics(&hir, &manifest);
    let e142 = only(&diags, DiagnosticCode::E142);
    assert!(
        e142.message.contains("annotate"),
        "E142 must demand an annotation: {}",
        e142.message
    );
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E144),
        "must never fall through to the old defensive never-visited refusal: {diags:?}"
    );
}

/// Review NIT on #2096's fix: the receiver inside a decl-default lambda can
/// itself be a file-scope GLOBAL (a `const` struct value), not a lambda
/// param — `head_ty`'s `Variable | Constant` arm reads `globals` by id
/// project-wide, so the decl-initializer walk must resolve it exactly as it
/// would outside a lambda. Pins that composition.
#[test]
fn a_const_valued_global_receiver_inside_a_decl_default_lambda_resolves() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

const guest: Guest = Guest{name: \"Ava\"}

const callGreet = || guest.greet(3)

fn main() {}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    let UfcsVerdict::FreeFnDesugar { receiver, name, .. } = &verdicts[0] else {
        panic!("expected a free-fn desugar verdict, got {:?}", verdicts[0]);
    };
    assert_eq!(name, "greet");
    assert_eq!(*receiver, brink_analyzer::Ty::Struct("Guest".into()));

    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E144)
            && !codes(&diags).contains(&DiagnosticCode::E142),
        "a const-valued global receiver must resolve, not demand annotation: {diags:?}"
    );
}

// ─── issue #1918: `FieldCall` argument checking ──────────────────────────
//
// #1881/PR #1914 added argument-type checking for the `FreeFnDesugar`/
// `FreeFnAutoRef` verdicts (the tests above) but deliberately left
// `FieldCall` — a call through a struct's own fn-typed field — uncovered
// (flagged in review on #1881, filed as #1918). `FieldCall` is structurally
// `strict::check_value_calls`'s T1c "call through a function value" domain
// (that check's own module doc), just reached via field access, so its own
// diagnostics are strict-mode-only (`E063`) exactly like that sibling —
// see `check_field_call_args`'s own doc in `ufcs.rs`.
//
// Every fixture here necessarily also carries an `E071` ("mistyped field")
// on the struct literal's own `greet: "hi"` initializer: the native surface
// has no first-class function-value literal yet (`#fn(target, args…)` is
// `brink-syntax`-only, T1c §2) — `brink-analyzer/tests/ufcs_resolution.rs`'s
// own `a_function_typed_field_wins_and_is_recorded_as_a_field_call` already
// tolerates this for the same reason, and `b3a_ufcs_e2e.rs`'s module doc
// tracks the grammar gap as a follow-up on #1505. Assertions below check
// `E063` specifically, never "diagnostics is empty", for exactly this
// reason.

/// Strict-mode whole-project diagnostics for one `.brink` file — the same
/// seam [`diagnostics`] wraps, but with `types = strict` forced
/// (`dialect = Brink`, so `resolve_type_policy` doesn't need an explicit
/// override; `is_native = true` so `strict::config_error`'s ink-only gate
/// is skipped). [`diagnostics`]'s own `AnalysisOptions::default()` resolves
/// to `dialect = StrictInk` → `types = Gradual` (`resolve_type_policy`'s
/// own doc), so it never reaches `strict::check` — this is the helper every
/// test below needs instead.
fn strict_diagnostics(hir: &HirFile, manifest: &SymbolManifest) -> Vec<Diagnostic> {
    let files = vec![(FileId(0), hir, manifest)];
    let analysis = brink_analyzer::analyze(&files);
    let opts = AnalysisOptions {
        dialect: brink_analyzer::Dialect::Brink,
        types: Some(brink_analyzer::TypePolicy::Strict),
        ..Default::default()
    };
    let (diags, _meta) = brink_analyzer::whole_project_diagnostics(
        &files,
        &analysis.index,
        &analysis.resolutions,
        &opts,
        // `is_native`: this is a real `.brink` fixture.
        true,
        None,
    );
    diags
}

/// A one-field-call fixture: `Guest` declares `greet: fn(int): int`, and
/// `main`'s body is `CALL_EXPR` (the call site under test).
fn field_call_fixture(call_expr: &str) -> (HirFile, SymbolManifest) {
    lower(&format!(
        "\
struct Guest {{
  greet: fn(int): int
}}

fn main() {{
  let g = Guest {{ greet: \"hi\" }};
  let n = {call_expr};
}}
"
    ))
}

/// A correctly-typed, correct-arity field call (`g.greet(3)` against
/// `greet: fn(int): int`) must resolve as a clean `FieldCall` verdict with
/// no `E063` under strict — the should-not-fire control every positive
/// check below needs.
#[test]
fn a_correctly_typed_field_call_reports_no_e063_under_strict() {
    let (hir, manifest) = field_call_fixture("g.greet(3)");
    let diags = strict_diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E063),
        "a correctly-typed, correct-arity field call must stay clean: {diags:?}"
    );
}

/// A field call supplying an argument of the wrong type
/// (`g.greet("nope")` against `greet: fn(int): int`) is a T1c-style
/// argument-type mismatch — `E063`, naming the field and the
/// expected/found types, matching `strict::check_value_calls`'s own
/// `ValueCallKind::ArgMismatch` phrasing ("call through", via
/// `field_call_arg_mismatch_diagnostic`, not the desugared-call "call to"
/// wording). The asserted "argument 1" also pins the index convention:
/// `UfcsArgMismatch::index` is 0-based over the *written* arguments for a
/// `FieldCall` (no receiver prepend — the Exception paragraph on that
/// field), so the first written argument reports as argument 1, not 2.
#[test]
fn a_field_call_with_a_mistyped_argument_is_reported_under_strict() {
    let (hir, manifest) = field_call_fixture("g.greet(\"nope\")");
    let diags = strict_diagnostics(&hir, &manifest);
    let e063 = only(&diags, DiagnosticCode::E063);
    assert!(
        e063.message
            .contains("argument 1 of call through `greet` has type `string`"),
        "E063 must use the T1c call-through phrasing, name the field, the mismatched type, and \
         the written-args-only argument number: {}",
        e063.message
    );
}

/// The should-not-fire control for the mistyped-argument case: the same
/// fixture, but under gradual mode (`AnalysisOptions::default()`, resolving
/// to `dialect = StrictInk` → `types = Gradual`) — `check_field_call_args`
/// computes the fact unconditionally, but `strict_verdict_diagnostics` only
/// ever runs from `strict::check`, so gradual mode must report nothing
/// (mirrors `external_binding_cross_kind_argument_is_not_checked_under_gradual`'s
/// own pattern in `strict.rs` for the same "computed unconditionally,
/// reported only under strict" posture).
#[test]
fn a_field_call_with_a_mistyped_argument_is_not_reported_under_gradual() {
    let (hir, manifest) = field_call_fixture("g.greet(\"nope\")");
    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E063),
        "gradual mode must never run the strict field-call argument check: {diags:?}"
    );
}

/// A field call with too few arguments (`g.greet()` against
/// `greet: fn(int): int`, one declared parameter) is an arity mismatch —
/// `E063`, phrased like `strict::check_value_calls`'s own
/// `ValueCallKind::ArityMismatch` ("call through `X` supplies N
/// argument(s) but its known type expects M"), naming the field.
///
/// This is the house-rule arity check itself: unlike `FreeFnDesugar`'s own
/// `E031` (unconditional, gradual included — `resolve::check_arity`'s own
/// convention for a call resolving straight to a known def), `FieldCall`'s
/// static arity check is strict-only by the same T1c posture as its
/// argument-*type* check above — gradual mode's own enforcement is the
/// runtime `FunctionValueArity` fault `Opcode::CallValue` already raises
/// for every call through a function value (the same bytecode shape this
/// verdict lowers to, `lower_ufcs_call`'s `FieldCall` arm) — see
/// `crates/internal/brink-test-harness/tests/tier1_brink.rs`'s own
/// `FunctionValueArity` tests for that mechanism proven end-to-end (through
/// `#fn`, the only surface that can construct a real function *value*
/// today — see `field_call_fixture`'s own doc comment above for why a
/// `FieldCall`-specific runtime playthrough isn't constructible yet).
#[test]
fn a_field_call_with_the_wrong_arity_is_reported_under_strict() {
    let (hir, manifest) = field_call_fixture("g.greet()");
    let diags = strict_diagnostics(&hir, &manifest);
    let e063 = only(&diags, DiagnosticCode::E063);
    assert!(
        e063.message.contains("greet") && e063.message.contains('1') && e063.message.contains('0'),
        "E063 must name the field and the expected/got counts: {}",
        e063.message
    );
}

/// The should-not-fire control for the arity case, gradual mode.
#[test]
fn a_field_call_with_the_wrong_arity_is_not_reported_under_gradual() {
    let (hir, manifest) = field_call_fixture("g.greet()");
    let diags = diagnostics(&hir, &manifest);
    assert!(
        !codes(&diags).contains(&DiagnosticCode::E063),
        "gradual mode must never run the strict field-call arity check: {diags:?}"
    );
}

/// Issue #2948 composition: `ufcs.rs`'s entry points switched to
/// `visit::visit_with_decl_initializers`, so a decl-default lambda's own
/// body now reaches this pass too (issue #2096) — a `FieldCall` site nested
/// inside one must get the exact same arity checking a knot/stitch body
/// gets. `check_field_call_args`'s arity half reads the call site's own
/// AST-derived `arg_count`, not `current_body()`'s `BodyTypes` projection
/// (which is `None` here — no enclosing knot/stitch — see that method's own
/// doc for why arity must not degrade alongside a body lookup it doesn't
/// depend on), so this must still fire.
#[test]
fn a_field_call_inside_a_decl_default_lambda_body_still_reports_wrong_arity_under_strict() {
    let (hir, manifest) = lower(
        "\
struct Guest {
  greet: fn(int): int
}

const callGreet = |g: Guest| g.greet()

fn main() {}
",
    );
    let verdicts = verdicts(&hir, &manifest);
    assert_eq!(verdicts.len(), 1, "one UFCS site: {verdicts:?}");
    assert!(
        matches!(verdicts[0], UfcsVerdict::FieldCall { .. }),
        "expected a field-call verdict, got {:?}",
        verdicts[0]
    );

    let diags = strict_diagnostics(&hir, &manifest);
    let e063 = only(&diags, DiagnosticCode::E063);
    assert!(
        e063.message.contains("greet"),
        "arity checking must still reach a field call inside a decl-default lambda body: {}",
        e063.message
    );
}
