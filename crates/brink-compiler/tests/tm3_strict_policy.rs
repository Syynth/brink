//! End-to-end TM-3 (#619) strict-policy tests (docs/typed-mode-spec.md
//! §1/§9-step-3). Mirrors `t1b_dialect_gate.rs`'s shape: exercises the full
//! pipeline through the public `brink_compiler::compile_with_options` entry
//! point — the concrete consumer path a CLI/library caller uses — proving
//! `AnalysisOptions::types` flows from the caller through `brink-driver` →
//! `brink-db`'s `lir_query` diagnostic gate → `CompileError::Diagnostics`,
//! and that a compile error under strict actually blocks `StoryData`
//! emission (not merely a reported warning).

#![allow(clippy::panic)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_ir::DiagnosticCode;

fn compile_mem(
    source: &str,
    dialect: Dialect,
    types: TypePolicy,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect,
        types: Some(types),
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
}

fn diagnostics_of(err: brink_compiler::CompileError) -> Vec<brink_compiler::ResolvedDiagnostic> {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => diags,
        other => panic!("expected Diagnostics error, got {other:?}"),
    }
}

fn compile_mem_manifest(
    source: &str,
    manifest: Option<brink_ir::HostManifest>,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        host_manifest: manifest,
        ..AnalysisOptions::default()
    };
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
}

/// A host manifest declaring `get_thing(id)` whose param `id` has type `ty`
/// (a `TypeRef` string). `types` registers `thing_id` as a scalar (base int)
/// semantic type so a `"thing_id"` `ty` resolves; pass `""` for an
/// unresolvable declared type.
fn get_thing_manifest(ty: &str) -> brink_ir::HostManifest {
    brink_ir::HostManifest {
        markup: Vec::new(),
        types: vec![brink_ir::SemanticTypeDef {
            name: "thing_id".to_string(),
            base: brink_ir::BaseType::Int,
            constraint: None,
            values: None,
            widget: None,
        }],
        externals: vec![brink_ir::ManifestExternal {
            name: "get_thing".to_string(),
            params: vec![brink_ir::ManifestParam {
                name: "id".to_string(),
                ty: brink_ir::TypeRef(ty.to_string()),
            }],
            returns: brink_ir::TypeRef("float".to_string()),
            kind: brink_ir::ExternalKind::default(),
            doc: None,
            widgets: Vec::new(),
            path: Vec::new(),
        }],
    }
}

// ── Issue #1004: external-declaration escape checking at the compile layer ──
//
// The compile pipeline's strict pass now escape-checks each registered
// `EXTERNAL`'s own params against the manifest signatures — the same
// `collect_external_sigs` resolution the analysis path seeds — through the
// shared `strict_diagnostics` seam. These pin the three regimes at the
// native `compile_with_options` layer (the wasm `compileProject` regression
// tests live in `brink-web`).

const EXT_SRC: &str =
    "EXTERNAL get_thing(id)\n=== start ===\n{get_thing(1) == 2:\n  yes\n}\n-> DONE\n";

#[test]
fn strict_manifest_typed_external_param_compiles_clean() {
    // A manifest `ty` naming a registered semantic type resolves the external
    // param — no escape, story compiles.
    let result = compile_mem_manifest(EXT_SRC, Some(get_thing_manifest("thing_id")));
    assert!(
        result.is_ok(),
        "a manifest-typed external param must not escape under strict: {result:?}"
    );
}

#[test]
fn strict_unresolvable_external_param_escapes_with_e065_at_decl_span() {
    // Registered, but the declared `ty` is empty — genuinely untyped, so it
    // escapes (don't-over-suppress guard), anchored at the external's own
    // `get_thing` declaration span (bytes 9..18 of `EXTERNAL get_thing(id)`).
    let err = compile_mem_manifest(EXT_SRC, Some(get_thing_manifest("")))
        .expect_err("an unresolvable external param type must fail strict compilation");
    let diags = diagnostics_of(err);
    let escape = diags
        .iter()
        .find(|d| d.code == DiagnosticCode::E065)
        .unwrap_or_else(|| panic!("expected an E065 escape: {diags:?}"));
    assert!(
        escape.message.contains("get_thing") && escape.message.contains("parameter `id`"),
        "escape must name the offending external param: {escape:?}"
    );
    assert_eq!(
        (escape.range.start().into(), escape.range.end().into()),
        (9u32, 18u32),
        "escape anchors at the external's own declaration span, not a fixed line: {escape:?}"
    );
}

#[test]
fn strict_unregistered_external_stays_unchecked() {
    // No manifest at all: the bare-identifier external params have no
    // in-language type source, so strict leaves them unchecked (compiles).
    let result = compile_mem_manifest(EXT_SRC, None);
    assert!(
        result.is_ok(),
        "an unregistered external's params must stay unchecked under strict: {result:?}"
    );
}

#[test]
fn default_types_is_gradual() {
    // No `types` set — `AnalysisOptions::default()` — under `dialect =
    // brink`, an unannotated, never-narrowed param compiles clean (gradual
    // is the byte-identical-forever floor).
    let result = compile_mem(
        "=== noop(x) ===\nHello.\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::default(),
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_plus_strict_ink_dialect_is_a_targeted_config_error() {
    let err = compile_mem(
        "=== noop(x) ===\nHello.\n-> DONE\n",
        Dialect::StrictInk,
        TypePolicy::Strict,
    )
    .expect_err("strict + strict-ink must fail to compile");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E064),
        "{diags:?}"
    );
}

#[test]
fn strict_plus_brink_dialect_blocks_compilation_on_unknown_escape() {
    let err = compile_mem(
        "=== noop(x) ===\nHello.\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("an unannotated, unused param must fail strict compilation");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E065),
        "{diags:?}"
    );
}

#[test]
fn strict_clean_project_compiles_to_story_data() {
    let result = compile_mem(
        "=== function heal(hp: int): int ===\n~ temp bonus: int = 5\n~ return hp + bonus\n\
         === main ===\nHello.\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_heterogeneous_collection_blocks_compilation() {
    // spec §5: `#[1, "a"]` is an error.
    let err = compile_mem(
        "=== main ===\n~ temp x = #[1, \"a\"]\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("heterogeneous collection literal must fail strict compilation");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E066),
        "{diags:?}"
    );
}

// ── Review fix 1: E063 is error-eligible under strict (#640-round ruling) ──

#[test]
fn strict_annotation_inference_mismatch_blocks_compilation_with_e063() {
    // A plain (non-function) knot, deliberately clean of every *other*
    // strict diagnostic: `x`'s annotation exempts it from Unknown-escape
    // (E065), and no return-type check applies (not a function knot) — the
    // only way this fixture can fail to compile is if E063 itself partitions
    // as an error. `x`'s annotation says `int`, but the body's own use
    // (`x + "!"`, string concatenation) forces a concrete `string` body type
    // — a genuine annotation-vs-inference disagreement, not an
    // Unknown/Conflicted body type (`mismatches()` skips unresolved body
    // types via `Ty::is_unresolved`, so the fixture must produce a
    // *concrete* disagreement to exercise E063 at all).
    //
    // Before this fix, `DiagnosticCode::E063.severity()` was hardcoded to
    // `Warning` and both `brink-db` partition sites split on that raw
    // severity — with no other error-severity diagnostic in this fixture,
    // `errors` would come out empty and this compile would have returned
    // `Ok` (with a warning), making `.expect_err` below panic. That is the
    // review's blocking finding.
    let err = compile_mem(
        "=== f(x: int) ===\n~ temp y = x + \"!\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "an annotation disagreeing with a concrete inferred body type must fail strict compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic \
         so E063 alone must be what fails compilation: {diags:?}"
    );
}

#[test]
fn gradual_annotation_inference_mismatch_still_compiles() {
    // Same fixture, `types` left at its default (`Gradual`) — E063 stays
    // advisory-only and never blocks compilation (the #618/PR#640 ruling
    // this issue explicitly does not touch).
    let result = compile_mem(
        "=== f(x: int) ===\n~ temp y = x + \"!\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::default(),
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── Review fix 2: void-assignment is an error under strict (spec §3) ──────

#[test]
fn strict_void_assignment_blocks_compilation_with_e067() {
    let err = compile_mem(
        "=== function noop(): void ===\n~ return\n\
         === main ===\n~ temp x = noop()\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("assigning a void call's result must fail strict compilation");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E067),
        "{diags:?}"
    );
}

#[test]
fn strict_inferred_void_assignment_blocks_compilation_with_e067() {
    // Issue #1054: `noop` here carries no `): void ===` annotation at all —
    // it's void purely by inference (#1046: no value-returning `return`
    // anywhere in its body). Must fail strict compilation with the same
    // `E067` the explicitly-annotated case above gets, through the same
    // real `compile_with_options` entry point.
    let err = compile_mem(
        "=== function noop() ===\nHello.\n\
         === main ===\n~ temp x = noop()\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("assigning an inferred-void call's result must fail strict compilation");
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E067),
        "{diags:?}"
    );
}

#[test]
fn strict_void_statement_position_call_compiles_clean() {
    // `~ f()` (no assignment) is never flagged — only the assignment/temp-
    // decl RHS-root shape is.
    let result = compile_mem(
        "=== function noop(): void ===\n~ return\n\
         === main ===\n~ noop()\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── TM-5 (#621) corpus wing growth: end-to-end (compile AND run) proofs ───
//
// The tests above prove strict-policy diagnostics fire/don't-fire at
// compile time; none of them actually *run* the compiled story to check
// runtime behavior is correct, not merely non-erroring. These two close
// that gap for the two spec §4 claims the upcoming book "Types" chapter
// makes verbatim: the condition-truthiness idiom survives strict (already
// unit-tested in `brink_analyzer::strict` for the type-checking side only),
// and `int -> float` is a real, directional, implicit coercion (not just
// "doesn't error").

#[test]
fn strict_condition_truthiness_idiom_runs_correctly_end_to_end() {
    // spec §4: "Condition-position int truthiness stays ({visited_knot: …},
    // nonzero = true) — scoped to condition position only." A knot's visit
    // count is a plain `int`; using it bare in a `{cond: …}` conditional
    // must type-check AND branch correctly under `types = strict` — the
    // idiom that makes ink ink, not merely a diagnostic-suppression rule.
    let src = "-> hub\n\
               === hub ===\n\
               { hub: I have been here before. | This is my first visit. }\n\
               -> hub_again\n\
               \n\
               === hub_again ===\n\
               { hub_again: Second time in this knot too. | First time here. }\n\
               -> DONE\n";
    let data = compile_mem(src, Dialect::Brink, TypePolicy::Strict)
        .expect("visit-count truthiness must compile clean under strict")
        .data;
    let (program, line_tables) = brink_runtime::link(&data).expect("link");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
        std::sync::Arc::new(program),
        line_tables,
    );
    let lines = story.continue_maximally().expect("run to completion");
    let mut text = String::new();
    for line in &lines {
        text.push_str(line.text());
    }
    // Ink increments a knot's visit count on *entry*, before its content
    // runs — so a self-referential `{knot: …}` check inside that very knot
    // is already looking at count 1 on the very first visit, hence the
    // "seen before" branch both times (verified against actual output: this
    // is the correct idiom's behavior, not a bug in this test's fixture).
    assert!(
        text.contains("I have been here before.") && text.contains("Second time in this knot too."),
        "{text:?}"
    );
}

#[test]
fn strict_int_to_float_coercion_runs_correctly_end_to_end() {
    // spec §4: "int -> float: implicit, directional (the one ink numeric
    // promotion)." An `int`-valued expression flowing into a `float`
    // binding must compile AND actually produce the promoted float value
    // at runtime, not merely fail to raise a coercion diagnostic.
    let src = "VAR rate: float = 1\n\
               ~ temp total: float = rate + 3\n\
               {total}\n\
               -> DONE\n";
    let data = compile_mem(src, Dialect::Brink, TypePolicy::Strict)
        .expect("int flowing into a float binding must compile clean under strict")
        .data;
    let (program, line_tables) = brink_runtime::link(&data).expect("link");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
        std::sync::Arc::new(program),
        line_tables,
    );
    let lines = story.continue_maximally().expect("run to completion");
    let mut text = String::new();
    for line in &lines {
        text.push_str(line.text());
    }
    assert_eq!(
        text.trim(),
        "4",
        "int 1 + int 3 promoted through a float binding"
    );
}

// ── F27 (issue #1120): condition-position Option[T] is E116 under strict ──

#[test]
fn strict_rejects_an_option_typed_condition_with_e116() {
    // F27 (docs/stdlib-spec.md §1.6, ruled 2026-07-19): Option has no
    // truthiness — the `{r: …}` guard NS-A1 blessed is now a compile error
    // under `types = strict` (and a runtime fault under gradual — see
    // `tier1_brink.rs`'s corpus twin).
    let err = compile_mem(
        "=== main ===\n~ temp r = find(\"ab\", \"b\")\n{r: found.}\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E116),
        "expected E116 for an Option-typed condition, got {diags:?}"
    );
}

#[test]
fn strict_rejects_a_direct_option_intrinsic_call_condition_with_e116() {
    // The direct-intrinsic shape — no temp in between.
    let err = compile_mem(
        "=== main ===\n{find(\"ab\", \"b\"): found.}\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E116),
        "expected E116 for a direct Option-returning call condition, got {diags:?}"
    );
}

#[test]
fn strict_accepts_explicit_option_comparisons_in_conditions() {
    // The blessed spellings compile clean AND run: `== some(x)` / `== none`.
    let data = compile_mem(
        "-> main\n=== main ===\n~ temp r = find(\"ab\", \"b\")\n{r == some(1): at one.}\n\
         {r == none: absent.}\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect("explicit Option comparisons must compile clean under strict")
    .data;
    let (program, line_tables) = brink_runtime::link(&data).expect("link");
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
        std::sync::Arc::new(program),
        line_tables,
    );
    let lines = story.continue_maximally().expect("run to completion");
    let text: String = lines.iter().map(brink_runtime::Step::text).collect();
    assert_eq!(text, "at one.\n");
}

// ── Issue #1864: direct-call argument types were never checked ─────────

#[test]
fn strict_direct_call_arg_mismatch_blocks_compilation_with_e063() {
    // `h`'s declared param `x` is `int`; the argument passed at the direct
    // call site `h("hi")` is a string literal — a genuine, statically-
    // provable disagreement, not an Unknown/Conflicted escape (those are
    // `mismatches`.rs's affair, already covered by E065/E066). Before this
    // fix (#1864), a direct call's arguments were only ever fed as
    // *evidence* into the argument's own inference, never checked against
    // the callee's already-known declared parameter type, so this fixture
    // compiled with zero diagnostics.
    let err = compile_mem(
        "=== function h(x: int) ===\n~ return\n\
         === main ===\n~ h(\"hi\")\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a direct call passing a string argument to a declared-int parameter must fail strict \
         compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn strict_direct_call_result_arg_mismatch_blocks_compilation_with_e063() {
    // The issue's own `take(mk())` shape: the argument is itself a *call*
    // result (`mk()` returns `string`), not a literal or a `~ temp`-backed
    // variable — `take`'s declared param `x` is `int`. Proves the check
    // reaches a call-valued argument too, via `structs::classify_expr_ty`'s
    // existing `Expr::Call` arm (the resolved callee's `InferredSig::
    // return_ty`), not just literal-shaped ones.
    let err = compile_mem(
        "=== function mk(): string ===\n~ return \"hi\"\n\
         === function take(x: int) ===\n~ return\n\
         === main ===\n~ take(mk())\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a direct call passing a string-returning call result to a declared-int parameter must \
         fail strict compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn strict_direct_call_arg_mismatch_via_conflicted_temp_reports_only_e066() {
    // Companion/negative case: when the mismatched argument is a `~ temp`
    // whose own type the call *itself* drives to `Ty::Conflicted` (the
    // pre-existing `InferPass::observe` join at this exact call site — see
    // `strict::check_escapes`'s Conflicted-escape, `E066`), this pass's own
    // `classify_expr_ty` reads the *finalized* (post-observe) local type,
    // sees `Conflicted`, and — matching the same "Unknown/Conflicted stays
    // silently unchecked" posture every other TM-3 mismatch check in this
    // crate follows — skips the argument rather than reporting a second,
    // redundant `E063` on top of the `E066` that already names the same
    // disagreement. A regression test for exactly this shape caught a
    // pre-existing `external_binding_rejects_cross_kind_handle_argument_
    // under_strict` unit test breaking during this fix's development (an
    // inline mid-walk check, since replaced by this post-hoc pass, could
    // observe the argument's type *before* that same call's own `observe`
    // join poisoned it, double-reporting E063 alongside E066).
    let err = compile_mem(
        "=== function h(x: int) ===\n~ return\n\
         === main ===\n~ temp s: string = \"hi\"\n~ h(s)\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("a Conflicted-escaping temp argument must still fail strict compilation");
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E066],
        "the temp's own Conflicted-escape must be the sole diagnostic here — no redundant \
         E063 on top of it: {diags:?}"
    );
}

#[test]
fn strict_direct_call_arg_mismatch_message_names_the_known_type_not_declared() {
    // Regression for a reviewer finding on #1864's original PR: the
    // diagnostic message said "its **declared** parameter type is `{}`",
    // but `expected` is read from `known_sigs`'s `InferredSig::params` —
    // the body-derived, *inferred* signature, not the annotation firewall.
    // The two disagree whenever a callee's own body has already driven a
    // param to a type its own annotation didn't say: here `outer(p:
    // string)` calls `h(p)`, and `h`'s sole param is `int`, so inference
    // widens `outer`'s *known* signature for `p` to `int` even though its
    // declared annotation stays `string`. `outer("a")` then mismatches
    // against the known (int) type, not the declared (string) one — the
    // old wording would have blamed the wrong type at this call site.
    let err = compile_mem(
        "=== function outer(p: string) ===\n~ h(p)\n~ return\n\
         === function h(x: int) ===\n~ return\n\
         === main ===\n~ outer(\"a\")\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a direct call whose known type disagrees with its declared annotation must \
                 still fail strict compilation",
    );
    let diags = diagnostics_of(err);
    let outer_call_diag = diags
        .iter()
        .find(|d| d.message.contains("call to `outer`"))
        .unwrap_or_else(|| panic!("expected a diagnostic naming the `outer` call site: {diags:?}"));
    assert!(
        !outer_call_diag.message.contains("declared"),
        "the message must not claim a specific *declared* type when `expected` is read from \
         the body-derived known signature: {}",
        outer_call_diag.message
    );
    assert!(
        outer_call_diag.message.contains("known type expects `int`"),
        "the message must name the known (inferred) type, matching check_value_calls's own \
         wording: {}",
        outer_call_diag.message
    );
}

#[test]
fn gradual_direct_call_arg_mismatch_still_compiles() {
    // Same fixture, `types` left at its default (`Gradual`) — the direct-
    // call check stays advisory-only and never blocks compilation, same
    // posture as every other TM-3 check (the runtime type-mismatch fault
    // is gradual mode's backstop).
    let result = compile_mem(
        "=== function h(x: int) ===\n~ return\n\
         === main ===\n~ h(\"hi\")\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::default(),
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── Issue #1877: VAR/CONST/temp initializers and assignments were never
// checked against declared type annotations (the remainder of #1864 that
// PR #1875's direct-call-argument check left) ──────────────────────────

#[test]
fn strict_var_initializer_annotation_mismatch_blocks_compilation_with_e063() {
    // The issue's own repro: `VAR v: int = "hi"` compiled with zero
    // diagnostics before this fix — the annotation *replaced* the
    // initializer's inferred type in `Sig::value_ty` (TM-2's firewall) but
    // was never checked against it.
    let err = compile_mem(
        "VAR v: int = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a VAR's string initializer disagreeing with its int annotation must fail \
                     strict compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn strict_const_initializer_annotation_mismatch_blocks_compilation_with_e063() {
    // CONST's own declaration site (`hir.constants`, a separate list from
    // `hir.variables`) — proves the check reaches both, not just VAR.
    let err = compile_mem(
        "CONST RATE: int = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a CONST's string initializer disagreeing with its int annotation must fail strict \
         compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn strict_content_var_initializer_never_coerces_from_string_with_e063() {
    // #1846's "content never coerces to or from string" invariant — the
    // original motivation named in #1877 — reaching a VAR initializer: a
    // bare string literal assigned to a `content`-annotated VAR must not
    // silently pass.
    let err = compile_mem(
        "VAR v: content = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a `content`-annotated VAR's string-literal initializer must fail strict compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn strict_temp_ascription_initializer_mismatch_blocks_compilation_with_e063() {
    // `~ temp t: int = "hi"` — the issue's own repro. The ascription is
    // recorded purely as an Unknown-escape fallback (never joined into the
    // Conflicted lattice), so this single-write disagreement reached no
    // other TM-3 check before this fix. Inside an explicit `=== main ===`
    // knot — TM-3's inference walk (and every check keyed on `inference.
    // bodies`) only ever reaches `hir.knots`, never `hir.root_content`
    // (bare top-level content before the first knot), so a bare `~ temp`
    // at file scope would never be inferred at all and this fixture would
    // vacuously pass regardless of whether the check itself worked.
    let err = compile_mem(
        "=== main ===\n~ temp t: int = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a `~ temp` initializer disagreeing with its own ascription must fail strict \
         compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn strict_var_assignment_mismatch_blocks_compilation_with_e063() {
    // `~ v = "hi"` on a declared `VAR v: int` — the issue's own repro.
    // Globals are never joined into the `Ty::Conflicted` lattice
    // (`infer::body`'s own module doc), so this was — and, absent this
    // check, would still be — entirely unchecked. The assignment itself
    // must live inside an explicit `=== main ===` knot (see the `~ temp`
    // test above's comment on why bare root content is never inferred);
    // the `VAR` declaration itself stays file-level, as ink grammar
    // requires.
    let err = compile_mem(
        "VAR v: int = 5\n=== main ===\n~ v = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a plain assignment disagreeing with a VAR's declared type must fail strict \
                 compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn strict_temp_reassignment_conflicted_reports_only_e066_not_e063() {
    // Companion/negative case, mirroring
    // `strict_direct_call_arg_mismatch_via_conflicted_temp_reports_only_e066`:
    // when a re-assignment to an annotated `~ temp` disagrees with a
    // *concrete* type that temp already carries (from its own initializer),
    // `InferPass::observe`'s join at this exact assignment already drives
    // the local to `Ty::Conflicted` on its own — independently reported as
    // `E066` by `strict::check_escapes`. `check_declared_assign_target`
    // must skip this write rather than double-reporting the identical
    // disagreement as `E063` too.
    let err = compile_mem(
        "=== main ===\n~ temp t: int = 5\n~ t = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("a Conflicted-escaping temp reassignment must still fail strict compilation");
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E066],
        "the temp's own Conflicted-escape must be the sole diagnostic here — no redundant \
         E063 on top of it: {diags:?}"
    );
}

#[test]
fn strict_unannotated_var_assignment_mismatch_blocks_compilation_with_e063() {
    // Review finding: `check_declared_assign_target`'s global arm reads
    // `ctx.globals`, whose declared type for an **unannotated** `VAR v = 5`
    // is still concrete — the initializer literal's own inferred type
    // (`collect_globals` reads `Sig::value_ty` the same way regardless of
    // whether an explicit `: type` annotation is present). So this check is
    // broader than "declared type annotations": it enforces cross-type
    // reassignment even on a `VAR` with no annotation at all.
    let err = compile_mem(
        "VAR v = 5\n=== main ===\n~ v = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a plain assignment disagreeing with an unannotated VAR's initializer-inferred type \
         must fail strict compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

// ── Review findings on this issue's own PR ──────────────────────────────

#[test]
fn strict_param_assignment_mismatch_reports_only_one_e063() {
    // Review finding: a Param assignment target used to *also* record a
    // `TypedAssignMismatch` fact in `check_declared_assign_target`, on top
    // of the `E063` `annotations::mismatches` already reports for the same
    // disagreement (the def's declared param annotation vs. the body's
    // *final* inferred param type — here, `p`'s single write makes its
    // final inferred type `string`, disagreeing with the `int` annotation).
    // Before the fix, this fixture reported the identical disagreement
    // twice, at two different spans (the annotation's, and the write's).
    // `check_declared_assign_target` now excludes `SymbolKind::Param`
    // entirely — a param annotation is a signature-firewall slot
    // `annotations::mismatches` already owns.
    let err = compile_mem(
        "=== function f(p: int) ===\n~ p = \"hi\"\n~ return 0\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a param assignment disagreeing with its own annotation must fail strict \
                 compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "exactly one E063 — the annotation-vs-final-inferred-type mismatch \
         `annotations::mismatches` already reports — not a second one from a redundant \
         per-write fact: {diags:?}"
    );
}

#[test]
fn strict_temp_later_read_conflict_reports_only_e066_not_e063() {
    // Review finding: `check_declared_assign_target`'s (and
    // `check_declared_temp_init`'s) per-write Conflicted guard only sees
    // whether *this* write is about to conflict its target — it cannot see
    // a *later* read that independently conflicts the same local further
    // down the body. Here `t`'s initializer (an unregistered external call)
    // infers `Unknown`, so `~ t = "hi"` is not itself a same-write conflict
    // and used to record an `E063` fact; the subsequent `t + true` then
    // joins `bool` against the temp's now-`string` type, driving it
    // `Conflicted` and independently reporting `E066` — double-reporting
    // the same disagreement. `infer_def_body`'s post-walk filter drops any
    // fact whose target's *final* whole-body type is `Conflicted`.
    //
    // Deliberately `t + true`, not `t + 1` (issue #1911): `string + int`
    // is now the display-concat carve-out this PR adds — a `string`/`int`
    // pairing here would leave `t` typed `string` (not `Conflicted`),
    // proving nothing about this double-reporting guard. `bool` stays
    // outside the carve-out (no `String`/`Bool` `Add` arm in
    // `value_ops::binary_op`), so it still drives a genuine `Conflicted`.
    let err = compile_mem(
        "EXTERNAL foo()\n=== function f() ===\n~ temp t: int = foo()\n~ t = \"hi\"\n\
         ~ return t + true\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a Conflicted-escaping temp reachable only via a later read must still fail \
                 strict compilation",
    );
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().all(|d| d.code == DiagnosticCode::E066),
        "only the temp's (and the def's) own Conflicted-escapes may fire here — no redundant \
         E063 alongside them: {diags:?}"
    );
}

#[test]
fn strict_temp_unknown_initializer_reassignment_blocks_compilation_with_e063() {
    // The residual case that is the entire stated justification for
    // `check_declared_assign_target`'s per-write guard on a Temp target
    // rather than a blanket kind exclusion: a `~ temp` whose own
    // initializer infers `Unknown` (here, an unregistered external call —
    // ink's grammar requires a `~ temp` to have *some* initializer
    // expression, so this is the reachable shape of "an as-yet-Unknown
    // local", not a literally initializer-less declaration). The first
    // write to `t` doesn't conflict on its own (`Unknown` unifies cleanly
    // with anything), so absent this check it would go completely
    // unchecked; with it, this is exactly the single-E063 case the guard is
    // for.
    let err = compile_mem(
        "EXTERNAL foo()\n=== function f() ===\n~ temp t: int = foo()\n~ t = \"hi\"\n\
         ~ return 0\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a temp reassignment disagreeing with its own ascription, reached only via an \
         Unknown-inferring initializer, must fail strict compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn gradual_var_initializer_annotation_mismatch_still_compiles() {
    // Same fixture as the VAR-initializer test above, `types` left at its
    // default (`Gradual`) — the new check stays advisory-only, same posture
    // as every other TM-3 check.
    let result = compile_mem(
        "VAR v: int = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::default(),
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn gradual_var_assignment_mismatch_still_compiles() {
    // Same fixture as the VAR-assignment test above, under `types =
    // gradual`.
    let result = compile_mem(
        "VAR v: int = 5\n=== main ===\n~ v = \"hi\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::default(),
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── Issue #1900: a *dotted* struct-field assignment target (`~ p.x =
// expr`) was excluded by #1899's own `check_declared_assign_target`
// (`p.segments.len() > 1` guard, this issue's split-off remainder) — the
// field's own declared type was never checked against the RHS at all, on
// either root source #1899's bare-name check covers (a global VAR/CONST or
// an annotated Param/Temp).

#[test]
fn strict_struct_field_assignment_mismatch_blocks_compilation_with_e063() {
    // The issue's own repro: `p.x`'s declared type (`float`, from `Point`'s
    // shape) disagrees with the RHS `string`.
    let err = compile_mem(
        "STRUCT Point = #{x: float, y: float}\n\
         VAR p: Point = Point#{x: 0.0, y: 0.0}\n\
         === main ===\n~ p.x = \"wrong\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "a dotted struct-field assignment disagreeing with the field's declared type must \
         fail strict compilation",
    );
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn strict_struct_field_assignment_of_declared_type_compiles_clean() {
    // Negative counterpart: the RHS's type agrees with `Point.x`'s declared
    // `float` (via the legal directional `int -> float` coercion, spec §4,
    // same as `structs::check`'s own E071 coercion test).
    let result = compile_mem(
        "STRUCT Point = #{x: float, y: float}\n\
         VAR p: Point = Point#{x: 0.0, y: 0.0}\n\
         === main ===\n~ p.x = 1\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_struct_field_assignment_on_annotated_temp_blocks_compilation_with_e063() {
    // The second of the issue's two named root sources: an annotated `~
    // temp`'s own ascription, not a global.
    let err = compile_mem(
        "STRUCT Point = #{x: float}\n\
         === main ===\n~ temp p: Point = Point#{x: 0.0}\n~ p.x = \"wrong\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("a dotted field assignment on an annotated temp must fail strict compilation");
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E063],
        "this fixture is deliberately clean of every other strict diagnostic so E063 alone \
         must be what fails compilation: {diags:?}"
    );
}

#[test]
fn gradual_struct_field_assignment_mismatch_still_compiles() {
    // Same fixture as the struct-field-assignment test above, under `types
    // = gradual` — the new check stays advisory-only, same posture as
    // every other TM-3 check.
    let result = compile_mem(
        "STRUCT Point = #{x: float, y: float}\n\
         VAR p: Point = Point#{x: 0.0, y: 0.0}\n\
         === main ===\n~ p.x = \"wrong\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::default(),
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── Issue #1911: `string + int`/`string + float` concatenation must not
// report E066 under strict types — this is ink's core display-concat
// idiom (spec §4, amended by this PR), and the runtime already accepts it
// unconditionally (`value_ops::binary_op`'s String/Int and String/Float
// `Add` arms). Reduced from the issue's own repro and from the native
// golden corpus fixture `tests/tier1-native/for-k-v/story.brink`
// (`sum_and_keys`, `keys + ":" + total`), which runs correctly today and
// must keep compiling clean under strict too.

#[test]
fn strict_string_plus_int_concat_compiles_clean() {
    let result = compile_mem(
        "=== function concat_int(): string ===\n\
         ~ temp total = 0\n\
         ~ total = total + 1\n\
         ~ return \"t:\" + total\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_int_plus_string_concat_compiles_clean() {
    // Same rule, reversed operand order (`int + string`, not just
    // `string + int`) — the runtime's `Add` arms are symmetric
    // (`value_ops::binary_op`), so the checker must be too.
    let result = compile_mem(
        "=== function concat_int(): string ===\n\
         ~ temp total = 0\n\
         ~ total = total + 1\n\
         ~ return total + \":t\"\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_string_plus_float_concat_compiles_clean() {
    let result = compile_mem(
        "=== function concat_float(): string ===\n\
         ~ temp total: float = 0.0\n\
         ~ total = total + 1.0\n\
         ~ return \"t:\" + total\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_float_plus_string_concat_compiles_clean() {
    // Missing-coverage review finding: the both-orders precedent is set by
    // `strict_string_plus_int_concat_compiles_clean`/
    // `strict_int_plus_string_concat_compiles_clean` above for the `int`
    // pairing, but `float + string` (as opposed to `string + float`) was
    // never exercised — `is_string_numeric_concat` covers it (`(Ty::Int |
    // Ty::Float, Ty::String)`), so this is a coverage gap only, not a
    // production fix.
    let result = compile_mem(
        "=== function concat_float(): string ===\n\
         ~ temp total: float = 0.0\n\
         ~ total = total + 1.0\n\
         ~ return total + \":t\"\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_chained_string_int_concat_compiles_clean() {
    // Mirrors the native corpus fixture exactly: `keys + ":" + total` is
    // `(string + string) + int`, left-associative.
    let result = compile_mem(
        "=== function sum_and_keys(): string ===\n\
         ~ temp total = 0\n\
         ~ temp keys = \"\"\n\
         ~ total = total + 1\n\
         ~ keys = keys + \"a\"\n\
         ~ return keys + \":\" + total\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── Review finding (BLOCKING): `+=` reaches the same string-numeric shape
// as infix `+` through a completely different pipeline seam — `Stmt::
// Assignment`/`BlockStmt::Assignment`'s `AssignOp::Add`, not `infer_infix`
// — and was still falsely rejecting under strict. `+=` desugars to the same
// runtime `Add` arm as `x = x + y` (`value_ops::binary_op`), so it must
// carry the identical string-numeric carve-out. Covers all three assignable
// target kinds `check_declared_assign_target`/`observe` distinguish: an
// unannotated `~ temp`, an annotated `~ temp: T`, and a global `VAR`.

#[test]
fn strict_temp_plus_eq_string_int_concat_compiles_clean() {
    let result = compile_mem(
        "=== function concat_temp(): string ===\n\
         ~ temp keys = \"k:\"\n\
         ~ temp total = 5\n\
         ~ keys += total\n\
         ~ return keys\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_annotated_temp_plus_eq_string_int_concat_compiles_clean() {
    let result = compile_mem(
        "=== function concat_annotated_temp(): string ===\n\
         ~ temp keys: string = \"k:\"\n\
         ~ temp total = 5\n\
         ~ keys += total\n\
         ~ return keys\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_var_plus_eq_string_int_concat_compiles_clean() {
    let result = compile_mem(
        "VAR keys = \"k:\"\n\
         === function concat_var(): string ===\n\
         ~ temp total = 5\n\
         ~ keys += total\n\
         ~ return keys\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── BLOCKING review finding (issue #1900): the `+=` string-numeric
// carve-out above must reach a *dotted* struct-field target too, not just a
// bare local/global. `Stmt::Assignment`'s own `is_string_numeric_concat_add`
// guard computes `target_ty` from `infer_expr(&a.target)`, which for a
// multi-segment `Path` only ever returns the ROOT's type (`Ty::Struct`,
// never the field's `string`/`int`) — so the guard can never trip for
// exactly the targets this PR's `check_declared_field_assign_target` newly
// checks. The carve-out has to be re-applied in
// `structs::check_field_assign_mismatch`, the only place the field's
// resolved declared type is ever known.

#[test]
fn strict_dotted_field_plus_eq_string_int_concat_compiles_clean() {
    let result = compile_mem(
        "STRUCT S = #{s: string}\n\
         VAR v: S = S#{s: \"a\"}\n\
         === main ===\n~ v.s += 5\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

#[test]
fn strict_dotted_field_plus_eq_int_field_string_concat_compiles_clean() {
    // Mirror case named in the same finding: an `int`-declared field with a
    // `string` RHS under `+=` is the same carve-out in the other type
    // direction.
    let result = compile_mem(
        "STRUCT S = #{n: int}\n\
         VAR v: S = S#{n: 1}\n\
         === main ===\n~ v.n += \"x\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    );
    assert!(result.is_ok(), "{result:?}");
}

// ── Review finding (issue #1900): a chained target (`o.i.a = v`, 3+
// segments) is unsupported at ANY RHS type — LIR's own
// `try_lower_field_assignment` rejects it outright with the
// non-suppressible `E074` regardless of whether the RHS's type agrees with
// the field. `check_declared_field_assign_target` is fenced at exactly
// `p.segments.len() == 2` (LIR's own single-level boundary) so a chained
// target never gets a `FieldAssignMismatch` fact recorded at all — this
// pins that a chained mistyped target reports `E074` alone, never a
// shadowed `E063` first (the pre-fix behavior the finding flagged as
// misleading: telling the author to fix a type on a construct that stays
// rejected either way).

#[test]
fn strict_chained_field_assignment_reports_only_e074_not_e063() {
    let err = compile_mem(
        "STRUCT Inner = #{a: int}\n\
         STRUCT Outer = #{i: Inner}\n\
         VAR o: Outer = Outer#{i: Inner#{a: 0}}\n\
         === main ===\n~ o.i.a = \"bad\"\n-> DONE\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err("a chained field-write target must fail strict compilation with E074");
    let diags = diagnostics_of(err);
    assert_eq!(
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagnosticCode::E074],
        "the chained target is rejected outright regardless of type — this must never surface \
         a shadowed E063 first: {diags:?}"
    );
}

#[test]
fn strict_string_plus_bool_still_blocks_compilation_with_e066() {
    // The display-concat carve-out is scoped to `Int`/`Float` only,
    // matching the runtime exactly: `value_ops::binary_op` has no
    // `String`/`Bool` `Add` arm, so this combination must still be a
    // genuine same-type conflict, not silently accepted.
    let err = compile_mem(
        "=== function concat_bool(): string ===\n\
         ~ temp flag = true\n\
         ~ return \"t:\" + flag\n",
        Dialect::Brink,
        TypePolicy::Strict,
    )
    .expect_err(
        "string + bool concatenation has no runtime support and must fail strict compilation",
    );
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E066),
        "{diags:?}"
    );
}
