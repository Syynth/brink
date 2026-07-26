//! Tier-1 brink corpus wing (`docs/t1b-surface-spec.md` §6, issue #570).
//!
//! Unlike `tests/tier{1,2,3}/`, this corpus has no C# oracle — vanilla ink
//! never had `~ { … }` blocks, sigil collection literals, or postfix
//! indexing, so there is nothing for inklecate to generate golden episodes
//! from. Each case under `tests/tier1-brink/<name>/` is `story.ink` (brink
//! dialect) plus a hand-written `expected.txt` derived directly from
//! `docs/t1b-surface-spec.md`'s semantics, not from any oracle. This test
//! compiles each case under `Dialect::Brink`, runs it to completion with the
//! deterministic `DotNetRng` (no choices in these cases — straight-line
//! programs are enough to exercise block/loop/collection/RMW lowering), and
//! asserts the concatenated output matches byte-for-byte.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Line, Story};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-brink")
}

/// Run `story.ink` (brink dialect) to completion and return the concatenated
/// output text. Panics (via `expect`/`unwrap` — test code, exempt per
/// `clippy.toml`) on any compile/runtime error, since every case here is
/// expected to succeed cleanly.
fn run_case(dir: &Path) -> String {
    let ink_path = dir.join("story.ink");
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let compile_msg = format!("compile {}", ink_path.display());
    let output = brink_compiler::compile_path_with_options(&ink_path, options).expect(&compile_msg);
    let link_msg = format!("link {}", ink_path.display());
    let (program, line_tables) = brink_runtime::link(&output.data).expect(&link_msg);
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    let step_msg = format!("runtime error in {}", ink_path.display());
    let mut out = String::new();
    let mut hit_choices = false;
    loop {
        match story.continue_single().expect(&step_msg) {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => {
                hit_choices = true;
                break;
            }
        }
    }
    assert!(
        !hit_choices,
        "{} presented choices — tier1-brink cases must be choice-free straight-line programs",
        ink_path.display()
    );
    out
}

fn assert_case(name: &str) {
    let dir = corpus_dir().join(name);
    let expected =
        brink_test_harness::corpus::load_golden_transcript(&dir.join("expected.txt"), name)
            .expect("golden transcript must be present and non-vacuous");
    let actual = run_case(&dir);
    assert_eq!(
        actual, expected,
        "case {name}: output mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
    );
}

#[test]
fn arrays_and_indexing() {
    assert_case("arrays-and-indexing");
}

#[test]
fn while_loop() {
    assert_case("while-loop");
}

#[test]
fn for_in_array() {
    assert_case("for-in-array");
}

#[test]
fn for_in_map_iterates_keys_in_insertion_order() {
    assert_case("for-in-map");
}

#[test]
fn nested_index_assignment_is_rmw() {
    assert_case("nested-index-assignment");
}

/// `memo[k] = v` on a fresh key inserts (JS/Python semantics) instead of
/// faulting `MapKeyNotFound` — issue #856, ruled 2026-07-15. Covers all
/// three of the ruling's acceptance shapes in one straight-line program:
/// fresh-key assign inserts (`"a"` then `"b"`), a repeat assign to the same
/// key overwrites in place rather than growing the map (`memo["a"] = 2`
/// after `memo["a"] = 1` leaves `len(memo) == 2`, not 3).
#[test]
fn map_index_assign_inserts_fresh_key() {
    assert_case("map-index-assign-inserts-fresh-key");
}

// ── #673: collection literals as VAR/CONST declaration defaults ─────────
//
// Deliberately does NOT use the `VAR p = 0` + reassignment workaround idiom
// this corpus otherwise relies on everywhere (`nested-index-assignment`'s
// precedent, and every stdlib-* case above) — the array/map literal here IS
// the declaration's actual default, proving `eval_const_expr` constant-folds
// it into a real `ConstValue::Array`/`Map` instead of silently compiling to
// `Null` (the pre-#673 bug).

#[test]
fn collection_literal_declaration_defaults_are_not_silently_null() {
    assert_case("collection-literal-declaration-defaults");
}

// ── TM-4c (#666): structs — construction/read/write, nesting, chains ─────

#[test]
fn struct_construct_read_write() {
    assert_case("struct-construct-read-write");
}

// ── #674: `arr[i].field = v` grammar fix ─────────────────────────────────
//
// The `.field` postfix grammar's assignment-target position used to reject
// an `Index` base entirely — `arr[0].x = 2.0` failed to parse as an
// assignment at all, producing a generic E015 parse error instead of
// reaching LIR's existing chained/mixed-field-write fence. This proves the
// full compiler entry point now surfaces the *intended* diagnostic — E074,
// the T1e boundary — not E015, for a mixed index-then-field write target.
// Full RMW support for this shape is still out of scope (T1e, deliberately
// deferred); this only fixes the grammar/diagnostic-routing gap.

#[test]
fn index_then_field_assignment_target_is_e074_not_a_parse_error() {
    let source = "VAR arr = #[1, 2, 3]\n~ arr[0].x = 2.0\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("a mixed index-then-field write target must still be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E074),
        "expected E074 (chained/mixed field-write projection), got {diags:?}"
    );
    assert!(
        diags
            .iter()
            .all(|d| d.code != brink_compiler::DiagnosticCode::E015),
        "must not regress to the old generic E015 parse error, got {diags:?}"
    );
}

// ── TM-5 (#621) corpus wing growth ────────────────────────────────────────
//
// TM-4c's `struct-construct-read-write` only reads/writes a struct in place;
// it never crosses a function-call boundary. This proves `Value::Record`
// marshals correctly as a call argument and a return value (real production
// codegen, not a unit test in isolation), and that the callee's returned
// struct is an independent copy — `p` must read back unchanged after
// `translate(p, ...)` returns a *new* point (value semantics, not aliasing).

/// NS-A3 (issue #1109, docs/stdlib-spec.md §9.6): the display protocol's
/// structural default for structs (`Point { x: 1, y: 2 }` — shape name +
/// fields in declared order), pinned on both consumers of the one display
/// path (F1): interpolation and `string()` render identically; nested
/// structs, structs-in-collections, and Option forms recurse through the
/// same path. `some(…)` still renders totally at both consumers (F28); a
/// final `none` at the interpolation boundary now renders as nothing
/// (`docs/stdlib-spec.md` §1.6b, Track B4) while `string(none)` keeps F28's
/// total `"none"` render — the fixture's `absent` / `absent via string`
/// pair proves the two consumers deliberately diverge for `None` only.
#[test]
fn struct_display_structural_default_serves_both_display_consumers() {
    assert_case("struct-display-default");
}

#[test]
fn struct_through_function_call_marshals_and_stays_a_value_copy() {
    assert_case("struct-through-function-call");
}

// ── TM-5 (#621) corpus wing growth: TM-2 inline annotations end-to-end ────
//
// TM-2 landed annotation grammar/HIR/fmt/IDE feeding `signature()`; this
//
// NOTE (scopeNotes, #680): the fixture deliberately writes its `temp` decl
// and its `ref`-argument call (`heal(gold, 10)`) as separate standalone `~`
// logic lines rather than inside one `~ { }` block. A `~ { temp … \n
// ref-call(global) }` block body — with or without any type annotation
// anywhere in the file — was found to misresolve the global's storage slot
// at runtime (`RuntimeError::UnresolvedGlobal`) while probing shapes for
// this fixture. Confirmed pre-existing (T1b-era, unrelated to annotations)
// and out of #621's corpus/book/IDE fence; filed as #680, not fixed here.
// proves the full annotation surface (§3: scalar `VAR`/`CONST`/`temp`
// ascriptions, `ref` params, a typed return, a `void`-returning function)
// compiles and runs through the real pipeline with the exact value an
// unannotated equivalent would produce — annotations are "optional
// seasoning" (spec §3), never a behavior change, proven at the corpus
// level rather than only in `brink-analyzer`'s unit tests.

#[test]
fn annotations_are_optional_seasoning_end_to_end() {
    assert_case("annotations-mixed");
}

#[test]
fn break_and_continue() {
    assert_case("break-continue");
}

#[test]
fn if_else_if_else_chain() {
    assert_case("if-else-chain");
}

// ── T1b-3 stdlib slice 1 (docs/t1b-surface-spec.md §5) ────────────────────

#[test]
fn stdlib_len_and_contains() {
    assert_case("stdlib-len-and-contains");
}

#[test]
fn stdlib_keys_and_values() {
    assert_case("stdlib-keys-and-values");
}

#[test]
fn stdlib_push_appends_in_call_order() {
    assert_case("stdlib-push");
}

#[test]
fn stdlib_insert_array_and_map() {
    assert_case("stdlib-insert");
}

#[test]
fn stdlib_remove_array_and_map() {
    assert_case("stdlib-remove");
}

#[test]
fn stdlib_mutator_accepts_an_indexed_path_lvalue() {
    assert_case("stdlib-mutator-nested-lvalue");
}

#[test]
fn stdlib_author_function_shadows_builtin() {
    assert_case("stdlib-shadowing");
}

// ── NS-A6: the std::rand draw verbs (docs/stdlib-spec.md §7, issue
// #1112). The expected.txt is a pinned-algorithm golden: seed(7) +
// DotNetRng fix every value, so any drift in the draw chain (seed
// derivation, 24-bit float shaping, Fisher-Yates order) fails loudly. ────

#[test]
fn rand_verbs_draw_deterministically_under_seed() {
    assert_case("rand-verbs");
}

// ── NS-A5: range values + the inhabited-range refinement
// (docs/stdlib-spec.md §7, F7/F8, issue #1111). Same pinned-algorithm
// golden discipline as rand-verbs: seed(11) + DotNetRng fix every draw. ──

#[test]
fn range_values_and_refinement_verbs() {
    assert_case("range-values");
}

// ── Stdlib slice 1 completion: char_at (docs/t1b-surface-spec.md §5,
// issue #857) ──────────────────────────────────────────────────────────────

#[test]
fn stdlib_char_at_indexes_chars_not_bytes() {
    assert_case("stdlib-char-at");
}

// ── TM-3 completion: conversion intrinsics (docs/typed-mode-spec.md §4,
// maintainer ruling 2026-07-13, issue #659) ───────────────────────────────

#[test]
fn stdlib_conversions_int_float_string() {
    assert_case("stdlib-conversions");
}

// ── #587 breadth pass: nesting depth (docs/t1b-surface-spec.md §2/§4) ────

#[test]
fn deep_nesting_control_flow() {
    assert_case("deep-nesting-control-flow");
}

#[test]
fn deep_nesting_collections() {
    assert_case("deep-nesting-collections");
}

// ── #587 breadth pass: weave<->block seam interleavings (§2's seam rule) ─

#[test]
fn weave_seam_two_knots() {
    assert_case("weave-seam-two-knots");
}

#[test]
fn weave_seam_stitch() {
    assert_case("weave-seam-stitch");
}

// ── #587 breadth pass: shadowing/scoping edges (§2) ───────────────────────

#[test]
fn shadowing_triple_nested_blocks() {
    assert_case("shadowing-triple-nested-blocks");
}

#[test]
fn shadowing_for_loop_variable() {
    assert_case("shadowing-for-loop-variable");
}

// ── #587 breadth pass: map key-domain edges post-#580 (value-model-spec §4) ─

#[test]
fn map_key_domain_contains_edges() {
    assert_case("map-key-domain-contains-edges");
}

// ── #587 breadth pass: RMW aliasing post-#576 (value-model-spec §5) ──────

#[test]
fn rmw_self_referential_flat_assignment() {
    assert_case("rmw-self-referential-flat-assignment");
}

#[test]
fn rmw_chain_self_referential() {
    assert_case("rmw-chain-self-referential");
}

#[test]
fn rmw_shared_map_cow() {
    assert_case("rmw-shared-map-cow");
}

#[test]
fn rmw_mutator_shared_nested_lvalue() {
    assert_case("rmw-mutator-shared-nested-lvalue");
}

// ── #680: ref-argument call co-occurring with a temp decl in one block ───

#[test]
fn ref_call_with_block_temp() {
    // The issue's literal repro shape end-to-end: a `~ { … }` block body
    // containing both a `temp` declaration and a `ref`-argument call
    // targeting a global `VAR`, with the temp used only within its own
    // block. Must resolve `gold`'s `ref` argument to the real global slot
    // and run to completion — see the RCA + regression tests in
    // `brink-ir`'s `lir_lowering.rs` for the root cause (a block-scoped
    // temp read *after* its block closes, unrelated to `ref` at all) and
    // the diagnostic (E082) that now catches that case instead.
    assert_case("ref-call-with-block-temp");
}

/// Every `tests/tier1-brink/` case directory is exercised by a `#[test]`
/// above — a directory with no matching test would silently never run.
#[test]
fn every_case_directory_has_a_test() {
    let known = [
        "arrays-and-indexing",
        "while-loop",
        "for-in-array",
        "for-in-map",
        "nested-index-assignment",
        "collection-literal-declaration-defaults",
        "break-continue",
        "if-else-chain",
        "stdlib-len-and-contains",
        "stdlib-keys-and-values",
        "stdlib-push",
        "stdlib-insert",
        "stdlib-remove",
        "stdlib-mutator-nested-lvalue",
        "stdlib-shadowing",
        "stdlib-char-at",
        "stdlib-conversions",
        "deep-nesting-control-flow",
        "deep-nesting-collections",
        "weave-seam-two-knots",
        "weave-seam-stitch",
        "shadowing-triple-nested-blocks",
        "shadowing-for-loop-variable",
        "map-key-domain-contains-edges",
        "map-index-assign-inserts-fresh-key",
        "rmw-self-referential-flat-assignment",
        "rmw-chain-self-referential",
        "rmw-shared-map-cow",
        "rmw-mutator-shared-nested-lvalue",
        "struct-construct-read-write",
        "struct-display-default",
        "struct-through-function-call",
        "annotations-mixed",
        "fn-value-call-forms",
        "fn-value-ref-mutation",
        "fn-value-bind-chain",
        "fn-value-bind-triple-chain",
        "ref-call-with-block-temp",
        "option-verbs",
        "rand-verbs",
        "range-values",
        "numeric-tower",
        "sort-verbs",
        "weighted-heap-verbs",
    ];
    let mut found: Vec<String> = std::fs::read_dir(corpus_dir())
        .expect("read tests/tier1-brink")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        // `algorithms/` (issue #822) is a nested sub-corpus, not a leaf
        // case directory — it owns its own `every_algorithms_case_
        // directory_has_a_test` invariant in `algorithms_corpus.rs`.
        .filter(|e| e.file_name() != "algorithms")
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut expected: Vec<String> = known.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    assert_eq!(found, expected, "add a #[test] for every case directory");
}

// ── E054 shadow-warning diagnostic (docs/t1b-surface-spec.md §2) ─────────

#[test]
fn block_scoped_temp_shadowing_an_outer_temp_warns() {
    let source = "~ {\n    temp x = 1\n    if true {\n        temp x = 2\n        x = x + 1\n    }\n}\nDone.\n-> END\n";
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    let out = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        options,
    )
    .expect("shadowing is a warning, not a compile error");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E054),
        "expected E054 shadow warning, got {:?}",
        out.warnings
    );
}

// ── T1b-3 stdlib diagnostics (docs/t1b-surface-spec.md §5) ───────────────

fn compile_brink(
    source: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        options,
    )
}

fn errors_of(err: &brink_compiler::CompileError) -> &[brink_compiler::ResolvedDiagnostic] {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => diags,
        other => panic!("expected Diagnostics error, got {other:?}"),
    }
}

/// NS-A9: the Brink dialect's unset-`types` default is now STRICT. Tests in
/// this file whose *subject* is the gradual regime's runtime semantics —
/// dynamic dispatch/arity/conversion faults the strict checker would reject
/// at compile time, or the `VAR x = 0` reassignment idiom — opt back into
/// gradual explicitly through these variants. Everything else was made
/// strict-clean with annotations instead (the per-fixture triage lives in
/// the NS-A9 PR table).
fn gradual_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(brink_compiler::TypePolicy::Gradual),
        ..AnalysisOptions::default()
    }
}

fn compile_and_link_gradual(
    source: &str,
) -> (
    std::sync::Arc<brink_runtime::Program>,
    Vec<Vec<brink_format::LineEntry>>,
) {
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    let output = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        gradual_opts(),
    )
    .expect("compile (explicit gradual)");
    let (program, tables) = brink_runtime::link(&output.data).expect("link");
    (std::sync::Arc::new(program), tables)
}

/// Gradual-regime counterpart of `run_expecting_fault` — for runtime faults
/// that only exist under gradual (strict rejects the same shape statically).
fn run_expecting_fault_gradual(source: &str) -> Option<brink_runtime::RuntimeError> {
    let (program, tables) = compile_and_link_gradual(source);
    let mut story = Story::<DotNetRng>::new(program, tables);
    for _ in 0..64 {
        match story.continue_single() {
            Ok(Line::Done { .. } | Line::End { .. } | Line::Suspended { .. }) => return None,
            Ok(_) => {}
            Err(e) => return Some(e),
        }
    }
    None
}

/// Gradual-regime counterpart of `run_to_error`.
fn run_to_error_gradual(source: &str) -> brink_runtime::RuntimeError {
    let (program, tables) = compile_and_link_gradual(source);
    let mut story = Story::<DotNetRng>::new(program, tables);
    loop {
        match story.continue_single() {
            Ok(Line::Text { .. }) => {}
            Ok(other) => panic!("expected a runtime fault, story completed with {other:?}"),
            Err(e) => return e,
        }
    }
}

#[test]
fn author_defined_len_shadows_builtin_with_e035_warning() {
    let source = "=== function len(x: int): int\n~ return 999\n\nHello.\n-> END\n";
    let out = compile_brink(source).expect("shadowing is a warning, not a compile error");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E035),
        "expected E035 shadow warning, got {:?}",
        out.warnings
    );
}

#[test]
fn push_with_an_rvalue_first_argument_is_a_compile_error() {
    // `push(#[1, 2], 3)` — the array literal is an rvalue, not a place to
    // write the mutated array back into.
    let source = "~ {\n    push(#[1, 2], 3)\n}\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("rvalue mutator argument must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E055),
        "expected E055, got {diags:?}"
    );
}

#[test]
fn insert_with_an_rvalue_first_argument_is_a_compile_error() {
    let source = "~ {\n    insert(#{\"a\": 1}, \"b\", 2)\n}\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("rvalue mutator argument must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E055),
        "expected E055, got {diags:?}"
    );
}

// ── #673: struct literal as a VAR/CONST declaration default ─────────────
//
// `eval_const_expr` has no `ConstValue` representation for a record (that's
// a format question outside this fix's fence) — a struct construction
// literal used directly as a declaration default is a real, non-suppressible
// compile error (E075) through the full `compile_with_options` entry point,
// never a silently-compiled `Null`.

#[test]
fn struct_literal_declaration_default_is_a_real_compile_error() {
    let source = "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n\nVAR p = Point#{x: 1.0, y: 2.0}\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("struct literal declaration default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E075),
        "expected E075, got {diags:?}"
    );
}

#[test]
fn map_literal_declaration_default_with_non_scalar_key_is_a_real_compile_error() {
    // Float is outside the ratified map-key domain (int/string/bool) — a
    // declaration default has no `MapNew` runtime-construction step to fault
    // at (unlike a mid-story map literal), so this must be a compile error.
    let source = "VAR m = #{3.5: 1}\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("non-scalar map key in a declaration default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E076),
        "expected E076, got {diags:?}"
    );
}

#[test]
fn array_literal_declaration_default_with_non_constant_element_is_a_real_compile_error() {
    // #679 review: a function call inside the literal can never
    // constant-fold — a declaration default is baked into `StoryData` at
    // compile time, with no runtime construction step left to evaluate the
    // element at. Before E077 the element silently compiled to `Null`
    // (#673's silent-Null bug one level down, inside the literal).
    let source = "VAR arr = #[f(), 2]\nHello.\n-> END\n\n=== function f()\n~ return 1\n";
    let err = compile_brink(source)
        .expect_err("non-constant array element in a declaration default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E077),
        "expected E077, got {diags:?}"
    );
}

#[test]
fn map_literal_declaration_default_with_non_constant_value_is_a_real_compile_error() {
    // #679 review: same E077 story as the array-element test, for a map
    // *value*. (A never-constant map *key* is already E076.)
    let source = "VAR m = #{\"a\": f()}\nHello.\n-> END\n\n=== function f()\n~ return 1\n";
    let err = compile_brink(source)
        .expect_err("non-constant map value in a declaration default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E077),
        "expected E077, got {diags:?}"
    );
}

// ── #692: bare non-constant scalar VAR/CONST declaration default ────────
//
// Sibling of the #673/#679 collection-element fixes above: a scalar
// `VAR`/`CONST` default whose *whole* value is a non-constant reference or
// call — not nested inside an array/map/struct/fn literal — previously
// silently folded to `Null` through `eval_const_expr`'s `Path`
// (`SymbolKind::Variable`) arm or its catch-all, with zero diagnostic. Now a
// real, non-suppressible compile error (E083) through the full
// `compile_with_options` entry point.

#[test]
fn var_declaration_default_referencing_another_var_is_a_real_compile_error() {
    let source = "VAR a = 1\nVAR b = a\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("a bare VAR-to-VAR declaration default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E083),
        "expected E083, got {diags:?}"
    );
}

#[test]
fn var_declaration_default_calling_a_function_is_a_real_compile_error() {
    let source = "VAR x = f()\nHello.\n-> END\n\n=== function f()\n~ return 1\n";
    let err = compile_brink(source)
        .expect_err("a bare function-call declaration default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E083),
        "expected E083, got {diags:?}"
    );
}

#[test]
fn const_declaration_default_referencing_a_var_is_a_real_compile_error() {
    let source = "VAR a = 1\nCONST b = a\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("a bare VAR-reference CONST declaration default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E083),
        "expected E083, got {diags:?}"
    );
}

#[test]
fn var_declaration_default_with_non_constant_wrapped_in_prefix_is_a_real_compile_error() {
    // Still a bare top-level default, not a collection element — `-f()`
    // recurses through the `Prefix` arm and must report the same E083 as an
    // unwrapped call.
    let source = "VAR x = -f()\nHello.\n-> END\n\n=== function f()\n~ return 1\n";
    let err = compile_brink(source)
        .expect_err("a non-constant prefix-wrapped declaration default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E083),
        "expected E083, got {diags:?}"
    );
}

#[test]
fn var_declaration_default_referencing_a_const_is_still_valid() {
    // No false positive: `SymbolKind::Constant` (unlike `Variable`) is a
    // genuine compile-time constant — `eval_const_expr`'s own `Path` arm
    // already resolves it via `const_values`.
    let source = "CONST a = 1\nVAR b = a\nHello.\n-> END\n";
    let out = compile_brink(source).expect("VAR referencing a CONST default must still compile");
    assert!(
        !out.warnings
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E083),
        "unexpected E083, got {:?}",
        out.warnings
    );
}

// ── #743: bare VAR reference nested one level inside a declaration-default
// collection/struct/fn literal — the residue #679's scope notes flagged and
// #692/E083 deliberately left alone (E083 governs only the *whole* default,
// not a construct nested one level in). `is_const_foldable_kind` (the
// element-level twin of `is_const_foldable_decl_default`) now resolves a
// nested `Path` the same way: a `SymbolKind::Variable` reference is never a
// compile-time constant, so it reports the standard `E077`, same as any
// other never-foldable element kind, instead of silently folding to `Null`
// with zero diagnostic.

#[test]
fn var_reference_nested_inside_an_array_default_is_now_a_real_compile_error() {
    let source = "VAR a = 1\nVAR arr = #[a]\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("a VAR reference nested inside an array default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E077),
        "expected E077, got {diags:?}"
    );
}

#[test]
fn var_reference_nested_inside_a_map_value_default_is_a_real_compile_error() {
    let source = "VAR a = 1\nVAR m = #{\"k\": a}\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("a VAR reference nested inside a map value default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E077),
        "expected E077, got {diags:?}"
    );
}

#[test]
fn var_reference_nested_inside_a_struct_field_default_is_still_e075() {
    // A struct construction literal used as a declaration default is
    // unconditionally E075 regardless of field content (`ConstValue` has no
    // record variant at all — #673) — a bare `VAR` field reference never
    // reaches the `E077` arm because the whole literal is already rejected.
    // Direct fixture proving the "struct field" position #743 names is
    // covered, not silently folded.
    let source = "STRUCT Point = #{\n    x: float,\n}\n\nVAR a = 1.0\nVAR p = Point#{x: a}\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("a VAR reference nested inside a struct field default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E075),
        "expected E075, got {diags:?}"
    );
}

#[test]
fn var_reference_nested_inside_a_bound_fn_val_arg_default_is_a_real_compile_error() {
    // `#fn(name, args…)`'s `val`-bound args are the fourth nested position
    // #743 names — `eval_const_fn_literal`'s `val` branch previously called
    // `eval_const_expr` directly with no `is_const_foldable_kind` gate at
    // all, so a bare `VAR` reference bound by value silently folded to
    // `Null` with zero diagnostic.
    let source = "=== function heal(hp) ===\n~ return hp + 1\n\nVAR g = 5\nVAR f = #fn(heal, g)\nHello.\n-> END\n";
    let err = compile_brink(source)
        .expect_err("a VAR reference bound by value in a #fn default must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E077),
        "expected E077, got {diags:?}"
    );
}

#[test]
fn const_reference_nested_inside_an_array_default_still_compiles_clean() {
    // Anti-regression: a `CONST` reference is a resolved `Path` that is
    // *not* `SymbolKind::Variable` — it must keep folding for real via
    // `const_values`, same as before #743, with no E077 false positive.
    let source = "CONST a = 1\nVAR arr = #[a]\nHello.\n-> END\n";
    let out = compile_brink(source)
        .expect("a CONST reference nested inside an array default must still compile");
    assert!(
        !out.warnings
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E077),
        "unexpected E077, got {:?}",
        out.warnings
    );
}

#[test]
fn mutator_used_in_expression_position_is_a_compile_error() {
    // Mutators return nothing (§5) — using the "result" is invalid.
    let source = "VAR arr = 0\n~ {\n    arr = #[]\n    temp x = push(arr, 1)\n}\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("mutator-as-expression must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E056),
        "expected E056, got {diags:?}"
    );
}

#[test]
fn stdlib_call_with_wrong_arity_warns() {
    // Arity mismatches are warnings throughout this codebase (E031 is also
    // how `resolve::check_arity` reports them for ordinary function calls)
    // — not a hard error, matching existing convention.
    let source = "VAR arr = 0\n~ {\n    arr = #[1]\n    temp n = len(arr, 1)\n}\nDone.\n-> END\n";
    let out = compile_brink(source).expect("wrong arity is a warning, not a compile error");
    assert!(
        out.warnings
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E031),
        "expected E031, got {:?}",
        out.warnings
    );
}

#[test]
fn strict_ink_rejects_an_unresolved_stdlib_call() {
    // Default dialect (StrictInk, no options override) never sees the
    // builtins — an unresolved `len(x)` call is a brink-extension error.
    let source = "VAR arr = 0\n~ x = len(arr)\nDone.\n-> END\n";
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    let err = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        AnalysisOptions::default(),
    )
    .expect_err("strict-ink must reject an unresolved stdlib call");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E051),
        "expected E051, got {diags:?}"
    );
}

// ── T1b-3 runtime faults (value-model-spec-spec §6, t1b-surface-spec §6) ──

fn run_to_error(source: &str) -> brink_runtime::RuntimeError {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_string()), options)
            .expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    loop {
        match story.continue_single() {
            Ok(Line::Text { .. }) => {}
            Ok(other) => panic!("expected a runtime fault, story completed with {other:?}"),
            Err(e) => return e,
        }
    }
}

#[test]
fn remove_array_index_out_of_bounds_faults() {
    let source = "VAR arr = 0\n~ {\n    arr = #[1, 2]\n    remove(arr, 5)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::IndexOutOfBounds { index: 5, len: 2 }
        ),
        "expected IndexOutOfBounds, got {err:?}"
    );
}

#[test]
fn insert_array_index_out_of_bounds_faults() {
    let source = "VAR arr = 0\n~ {\n    arr = #[1, 2]\n    insert(arr, 9, 3)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::IndexOutOfBounds { index: 9, len: 2 }
        ),
        "expected IndexOutOfBounds, got {err:?}"
    );
}

#[test]
fn contains_on_a_non_collection_faults() {
    let source = "VAR n = 5\n~ {\n    temp b = contains(n, 5)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotIndexable("int")),
        "expected NotIndexable, got {err:?}"
    );
}

// ── #587 breadth pass: every stdlib function x faults (§5, value-model §11c) ─
//
// `contains`/`push`/`insert`/`remove` on non-collection/out-of-bounds roots
// are covered above and in `take_rmw.rs`. This section rounds out `len`,
// `keys`, `values` (including the `values(array)` edge, which faults —
// `collection_values` is Map-only, unlike `collection_keys`'s deliberate
// array-identity pass-through documented on `collection_ops::collection_keys`).

#[test]
fn len_on_a_non_collection_faults() {
    let source = "VAR n = 5\n~ {\n    temp x = len(n)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotIndexable("int")),
        "expected NotIndexable, got {err:?}"
    );
}

#[test]
fn keys_on_a_non_collection_faults() {
    let source = "VAR n = 5\n~ {\n    temp x = keys(n)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotIndexable("int")),
        "expected NotIndexable, got {err:?}"
    );
}

#[test]
fn keys_on_an_array_is_identity_pass_through_not_a_fault() {
    // `collection_keys`'s documented array branch: "returns the array itself
    // unchanged" — the single-opcode `for x in iterable` unification (§2/§5)
    // depends on this being total for arrays, not a fault.
    let source = "VAR arr = 0\nVAR out = \"\"\n~ {\n    arr = #[7, 8, 9]\n    for x in keys(arr) {\n        out = out + \" \" + x\n    }\n}\n{out}\n-> END\n";
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_string()), options)
            .expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut out = String::new();
    loop {
        match story.continue_single().expect("no fault expected") {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => panic!("unexpected choices"),
        }
    }
    assert_eq!(out.trim(), "7 8 9");
}

#[test]
fn values_on_an_array_faults() {
    // Unlike `keys`, `collection_values` is Map-only — no array pass-through.
    let source =
        "VAR arr = 0\n~ {\n    arr = #[1, 2, 3]\n    temp x = values(arr)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotIndexable("array")),
        "expected NotIndexable(\"array\"), got {err:?}"
    );
}

#[test]
fn values_on_a_non_collection_faults() {
    let source = "VAR n = 5\n~ {\n    temp x = values(n)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotIndexable("int")),
        "expected NotIndexable, got {err:?}"
    );
}

#[test]
fn push_on_a_non_collection_faults() {
    let source = "VAR arr = 0\n~ {\n    arr = 5\n    push(arr, 1)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotIndexable("int")),
        "expected NotIndexable, got {err:?}"
    );
}

// ── #587 breadth pass: map key-domain edges post-#580 ─────────────────────
//
// `contains(m, needle)` is TOTAL on a non-key-domain needle — `false`, never
// a fault (#580, `map-key-domain-contains-edges` corpus case). Indexing and
// the `insert`/`remove` mutators are NOT total on a non-key-domain key —
// `to_map_key` faults with `InvalidMapKeyType` for all of them, unchanged by
// #580 (only the `MapContains` map branch's call site changed). This
// asymmetry is the exact edge value-model-spec §11c draws: `contains` has no
// "the key isn't there" failure mode to escalate to a fault, but
// indexing/`insert`/`remove` do.

// ── #856: map[newKey] = value inserts, but reads still fault ─────────────
//
// Ruled 2026-07-15: indexed *assignment* to an absent map key now inserts
// (JS/Python semantics — `map-index-assign-inserts-fresh-key` above), but
// this doesn't loosen reads (`m[k]`, `IndexGet`) at all — those still fault
// `MapKeyNotFound` on a missing key, exactly as before, per
// value-model-spec §11c.

#[test]
fn map_index_get_absent_key_still_faults() {
    let source =
        "VAR m = 0\n~ {\n    m = #{\"a\": 1}\n    temp x = m[\"absent\"]\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::MapKeyNotFound { ref key } if key == "absent"),
        "expected MapKeyNotFound {{ key: \"absent\" }}, got {err:?}"
    );
}

#[test]
fn map_index_get_with_non_key_domain_float_faults() {
    let source = "VAR m = 0\n~ {\n    m = #{\"a\": 1}\n    temp x = m[3.5]\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::InvalidMapKeyType("float")),
        "expected InvalidMapKeyType(\"float\"), got {err:?}"
    );
}

#[test]
fn map_index_set_with_non_key_domain_float_faults() {
    let source = "VAR m = 0\n~ {\n    m = #{\"a\": 1}\n    m[3.5] = 9\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::InvalidMapKeyType("float")),
        "expected InvalidMapKeyType(\"float\"), got {err:?}"
    );
}

#[test]
fn map_insert_with_non_key_domain_float_key_faults() {
    let source = "VAR m = 0\n~ {\n    m = #{\"a\": 1}\n    insert(m, 3.5, 9)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::InvalidMapKeyType("float")),
        "expected InvalidMapKeyType(\"float\"), got {err:?}"
    );
}

#[test]
fn map_remove_with_non_key_domain_float_key_faults() {
    let source = "VAR m = 0\n~ {\n    m = #{\"a\": 1}\n    remove(m, 3.5)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert!(
        matches!(err, brink_runtime::RuntimeError::InvalidMapKeyType("float")),
        "expected InvalidMapKeyType(\"float\"), got {err:?}"
    );
}

// ── #587 breadth pass: every stdlib function x the dialect gate (§1/§5) ───
//
// `dialect_gate.rs`'s unit tests prove the gate mechanism generically via
// one name (`len`) plus the "resolved call is never flagged" shadow case;
// this proves the full seven-name surface end-to-end through the compiler
// entry point, matching `strict_ink_rejects_an_unresolved_stdlib_call`'s
// existing single-name version above.

// ── TM-3 completion: conversion intrinsics (docs/typed-mode-spec.md §4,
// maintainer ruling 2026-07-13, issue #659) ───────────────────────────────

#[test]
fn author_defined_int_shadows_builtin_with_e035_warning() {
    let source = "=== function int(x: int): int\n~ return 999\n\nHello.\n-> END\n";
    let out = compile_brink(source).expect("shadowing is a warning, not a compile error");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E035),
        "expected E035 shadow warning, got {:?}",
        out.warnings
    );
}

#[test]
fn every_conversion_name_is_rejected_under_strict_ink_and_compiles_under_brink() {
    let strict_ink_call_sites: [(&str, &str); 3] = [
        ("int", "int(x)"),
        ("float", "float(x)"),
        ("string", "string(x)"),
    ];
    for (name, call) in strict_ink_call_sites {
        let source = format!("VAR x = 0\n~ y = {call}\nDone.\n-> END\n");
        let files: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::from([("main.ink", source.as_str())]);
        let err = brink_compiler::compile_with_options(
            "main.ink",
            |path| {
                files
                    .get(path)
                    .map(|s| (*s).to_string())
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
            },
            AnalysisOptions::default(),
        )
        .expect_err(&format!(
            "strict-ink must reject an unresolved `{name}` call"
        ));
        let diags = errors_of(&err);
        assert!(
            diags
                .iter()
                .any(|d| d.code == brink_compiler::DiagnosticCode::E051),
            "`{name}`: expected E051, got {diags:?}"
        );
    }

    for (name, expr) in [
        ("int", "int(1)"),
        ("float", "float(1.0)"),
        ("string", "string(1)"),
    ] {
        let brink_source = format!("~ temp y = {expr}\nDone.\n-> END\n");
        compile_brink(&brink_source)
            .unwrap_or_else(|e| panic!("`{name}` must compile under brink dialect: {e:?}"));
    }
}

#[test]
fn int_parse_failure_faults() {
    let source = "~ {\n    temp x = int(\"potato\")\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert_eq!(
        err,
        brink_runtime::RuntimeError::ConversionParseFailure {
            target: "int",
            input: "potato".to_string(),
        }
    );
}

#[test]
fn float_parse_failure_faults() {
    let source = "~ {\n    temp x = float(\"nope\")\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert_eq!(
        err,
        brink_runtime::RuntimeError::ConversionParseFailure {
            target: "float",
            input: "nope".to_string(),
        }
    );
}

#[test]
fn int_of_negative_float_truncates_toward_zero_not_floor() {
    // Ruling 3's pinned case: int(-2.9) == -2 (truncate), not -3 (floor) —
    // matches vanilla ink's INT() exactly.
    let source = "{int(-2.9)}\n-> END\n";
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_string()), options)
            .expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    match story.continue_single().expect("no fault expected") {
        Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
            assert_eq!(text.trim(), "-2");
        }
        other => panic!("expected terminal line, got {other:?}"),
    }
}

#[test]
fn int_of_a_divert_target_faults_under_gradual() {
    // Root-level content is the entry point (`run_to_error`'s pattern
    // throughout this file) — `target` is declared only so `-> target` has
    // somewhere to resolve; it's never actually reached. NS-A9: gradual
    // explicitly — the strict regime rejects this same shape at compile
    // time as E078 (`strict_mode_rejects_int_of_a_divert_target_literal_
    // with_e078` below is that companion).
    let source =
        "~ {\n    temp x = int(-> target)\n}\nDone.\n-> END\n=== target ===\nHi.\n-> DONE\n";
    let err = run_to_error_gradual(source);
    assert_eq!(
        err,
        brink_runtime::RuntimeError::InvalidConversionDomain {
            target: "int",
            got: "divert_target",
        }
    );
}

#[test]
fn float_of_a_list_faults_under_gradual() {
    // NS-A9: gradual explicitly — strict rejects this as E078 at compile.
    let source = "LIST Colors = red, blue\n~ {\n    temp x = float((red))\n}\nDone.\n-> END\n";
    let err = run_to_error_gradual(source);
    assert_eq!(
        err,
        brink_runtime::RuntimeError::InvalidConversionDomain {
            target: "float",
            got: "list",
        }
    );
}

#[test]
fn int_of_an_array_faults_under_gradual() {
    let source = "VAR arr = 0\n~ {\n    arr = #[1, 2]\n    temp x = int(arr)\n}\nDone.\n-> END\n";
    let err = run_to_error(source);
    assert_eq!(
        err,
        brink_runtime::RuntimeError::InvalidConversionDomain {
            target: "int",
            got: "array",
        }
    );
}

#[test]
fn string_of_a_divert_target_never_faults() {
    // Ruling 2: `string()` accepts every type — the same divert-target
    // input that faults `int()` above must succeed for `string()`. Root
    // content is the entry point; `target` is declared only so `-> target`
    // resolves, never actually reached.
    let source = "VAR s = \"\"\n~ {\n    s = string(-> target)\n}\n{s}\n-> END\n=== target ===\nHi.\n-> DONE\n";
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_string()), options)
            .expect("compile");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut out = String::new();
    loop {
        match story.continue_single().expect("string() must never fault") {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => panic!("unexpected choices"),
        }
    }
    // Proves the conversion actually ran (not a vacuous pass on zero
    // executed lines) — `string(-> target)`'s display form is non-empty.
    assert!(
        !out.trim().is_empty(),
        "expected string(-> target)'s non-empty display form in output, got {out:?}"
    );
}

#[test]
fn strict_mode_rejects_int_of_a_divert_target_literal_with_e078() {
    let source = "=== knot ===\nHello.\n-> DONE\n=== main ===\n~ x = int(-> knot)\n-> DONE\n";
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(brink_compiler::TypePolicy::Strict),
        ..AnalysisOptions::default()
    };
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    let err = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        options,
    )
    .expect_err("strict mode must reject int(-> knot) at compile time");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E078),
        "expected E078, got {diags:?}"
    );
}

#[test]
fn every_stdlib_name_is_rejected_under_strict_ink_and_compiles_under_brink() {
    // Pure functions are gated as a plain unresolved call outside a block;
    // mutators (§5: "require an lvalue first argument") are gated the same
    // way — `is_t1b_stdlib_call_name` doesn't distinguish pure/mutator, only
    // resolution status, so an unresolved `push(arr)` call is flagged
    // exactly like `len(arr)` regardless of arity/lvalue-ness (those checks
    // are downstream, brink-dialect-only concerns E055/E058).
    let strict_ink_call_sites: [(&str, &str); 7] = [
        ("len", "len(arr)"),
        ("keys", "keys(arr)"),
        ("values", "values(arr)"),
        ("contains", "contains(arr, 1)"),
        ("push", "push(arr)"),
        ("insert", "insert(arr)"),
        ("remove", "remove(arr)"),
    ];
    for (name, call) in strict_ink_call_sites {
        let source = format!("VAR arr = 0\n~ x = {call}\nDone.\n-> END\n");
        let files: std::collections::HashMap<&str, &str> =
            std::collections::HashMap::from([("main.ink", source.as_str())]);
        let err = brink_compiler::compile_with_options(
            "main.ink",
            |path| {
                files
                    .get(path)
                    .map(|s| (*s).to_string())
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
            },
            AnalysisOptions::default(),
        )
        .expect_err(&format!(
            "strict-ink must reject an unresolved `{name}` call"
        ));
        let diags = errors_of(&err);
        assert!(
            diags
                .iter()
                .any(|d| d.code == brink_compiler::DiagnosticCode::E051),
            "`{name}`: expected E051, got {diags:?}"
        );
    }

    // Under `Dialect::Brink`, each name resolves and lowers cleanly with a
    // signature-correct call: pure functions as an expression, mutators as
    // an lvalue-first statement (§5).
    let brink_call_sites: [(&str, &str); 7] = [
        ("len", "temp x = len(arr)"),
        ("keys", "temp x = keys(arr)"),
        ("values", "temp x = values(m)"),
        ("contains", "temp x = contains(arr, 1)"),
        ("push", "push(arr, 3)"),
        ("insert", "insert(arr, 0, 9)"),
        ("remove", "remove(arr, 0)"),
    ];
    for (name, stmt) in brink_call_sites {
        let brink_source = format!(
            "VAR arr = 0\nVAR m = 0\n~ {{\n    arr = #[1, 2]\n    m = #{{\"a\": 1}}\n    {stmt}\n}}\nDone.\n-> END\n"
        );
        compile_brink(&brink_source)
            .unwrap_or_else(|e| panic!("`{name}` must compile under brink dialect: {e:?}"));
    }
}

// ── T1c-2 (#700): `#fn(…)` function values — real lowering ──────────────
//
// T1c-2 lands LIR/codegen/VM (docs/t1c-spec.md §11): the T1c-1 E052 lowering
// fence is retired. A program that uses `#fn` under `dialect = brink` now
// compiles clean (expression position AND declaration defaults) — never a
// silent drop, never a fence error.

#[test]
fn fn_literal_under_brink_dialect_lowers_for_real() {
    let source = "=== function heal(hp) ===\n~ return hp + 1\n\n\
                  === main ===\n~ temp f = #fn(heal, 1)\nDone.\n-> END\n";
    compile_brink(source).expect("#fn now lowers for real in T1c-2 (no E052 fence)");
}

#[test]
fn fn_literal_declaration_default_bakes_a_real_value_not_a_null() {
    // A `VAR` default has no runtime construction step — T1c-2 bakes a real
    // `FnRef`/`Closure` into StoryData (never the silent `Null` the house
    // rules forbid, and no E052 fence).
    let source = "=== function heal(hp) ===\n~ return hp + 1\n\n\
                  VAR f = #fn(heal, 1)\nHello.\n-> END\n";
    compile_brink(source).expect("#fn as a declaration default compiles clean in T1c-2");
}

// ── T1c-1 (#699): creation-site diagnostics E079/E080/E081 ───────────────

#[test]
fn fn_target_that_is_not_a_function_definition_is_e079() {
    let source = "VAR gold = 5\n=== main ===\n~ temp f = #fn(gold)\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("#fn(variable) must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E079),
        "expected E079, got {diags:?}"
    );
}

#[test]
fn fn_ref_param_bound_to_temp_is_e080() {
    let source = "=== function heal(ref hp, amount) ===\n~ hp = hp + amount\n~ return hp\n\n\
                  === main ===\n~ temp local_hp = 10\n~ temp f = #fn(heal, local_hp)\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("ref bound to a temp must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E080),
        "expected E080, got {diags:?}"
    );
}

#[test]
fn fn_unbound_ref_param_is_e080() {
    let source = "=== function heal(ref hp, amount) ===\n~ hp = hp + amount\n~ return hp\n\n\
                  === main ===\n~ temp f = #fn(heal)\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("unbound ref param must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E080),
        "expected E080, got {diags:?}"
    );
}

#[test]
fn fn_over_binding_is_e081() {
    let source = "=== function double(x) ===\n~ return x + x\n\n\
                  === main ===\n~ temp f = #fn(double, 1, 2)\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("over-binding must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E081),
        "expected E081, got {diags:?}"
    );
}

#[test]
fn well_formed_fn_creation_compiles_clean() {
    // A fully legal creation site under gradual types compiles clean in T1c-2
    // — no E079/E080/E081 creation noise and no E052 fence.
    let source = "=== function heal(ref hp: int, amount: int): int ===\n~ hp = hp + amount\n~ return hp\n\n\
                  VAR player_hp = 10\n=== main ===\n~ temp f = #fn(heal, player_hp)\nDone.\n-> END\n";
    compile_brink(source).expect("a well-formed #fn creation compiles clean in T1c-2");
}

// ── #680 RCA: block-scoped temp referenced after its block closes (E082) ──
//
// #680 was filed as "a `ref`-argument call co-occurring with a `temp` decl
// in the same `~ { }` block resolves to the wrong global slot
// (UnresolvedGlobal)". RCA (see `brink-ir`'s `lir_lowering.rs` for the
// lowering-layer regression tests) found the `ref`-argument call is not the
// trigger at all: the actual defect is reading a T1b block-scoped `temp`
// from *outside* its own `~ { … }` block, which used to silently fall
// through to the compiler's inklecate-compat "forward-referenced classic
// temp" path and emit a phantom global id — a runtime-only
// `UnresolvedGlobal` fault with no compile diagnostic. That's now a real,
// non-suppressible compile error (E082) instead.

#[test]
fn block_scoped_temp_read_after_its_block_closes_is_e082() {
    let source = "VAR gold = 100\n~ {\n    temp name = \"hero\"\n}\n{name}\n-> END\n";
    let err =
        compile_brink(source).expect_err("reading a block temp after its block closed must fail");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E082),
        "expected E082, got {diags:?}"
    );
}

#[test]
fn block_scoped_temp_passed_by_ref_after_its_block_closes_is_e082() {
    let source = "VAR gold = 100\n~ {\n    temp name = \"hero\"\n}\n~ heal(name)\nDone.\n-> END\n\n\
                  === function heal(ref hp: string): string ===\n~ return hp\n";
    let err =
        compile_brink(source).expect_err("passing an out-of-scope block temp by ref must fail");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E082),
        "expected E082, got {diags:?}"
    );
}

#[test]
fn ref_call_co_occurring_with_block_temp_is_not_e082() {
    // The issue's literal repro shape: a `ref`-argument call and a `temp`
    // decl in the *same* block, temp never read after the block closes —
    // this compiles clean, proving `ref` calls were never the trigger.
    let source = "VAR gold = 100\n~ {\n    temp x = 1\n    heal(gold)\n}\nDone.\n-> END\n\n\
                  === function heal(ref hp: int): int ===\n~ return hp\n";
    compile_brink(source)
        .expect("ref-argument call co-occurring with a temp decl in one block compiles clean");
}

// ── T1c-2 (#700): function-value corpus wing (straight-line cases) ───────

#[test]
fn fn_value_both_call_forms() {
    // Creation (zero-bound FnRef + bound Closure) and both call forms — the
    // direct `f(args…)` and the explicit `call(f, args…)`.
    assert_case("fn-value-call-forms");
}

#[test]
fn fn_value_ref_cell_mutation_through_a_stored_value() {
    // A `#fn(heal, player_hp)` ref-binds a durable World cell; invoking the
    // stored value twice mutates the cell through the captured pointer.
    assert_case("fn-value-ref-mutation");
}

// ── T1c-3 (#701): `bind` currying + display form ────────────────────────

#[test]
fn fn_value_bind_chains_and_display_form() {
    // `bind(f, args…)` val-only currying — nested bind chains, `bind` then
    // `call`, `bind` over a ref-bound value, and the authoritative `string(f)`
    // display form (`fn heal(ref hp = world_hp, amount)`, spec §5) exercised
    // through interpolation. All through real production codegen.
    assert_case("fn-value-bind-chain");
}

// ── T1c-4 (#702): mechanical tail — deeper bind-of-bind corpus growth ────

#[test]
fn fn_value_bind_of_bind_fully_curries_to_a_zero_arg_call() {
    // `fn-value-bind-chain` already proves a two-level bind-of-bind
    // (`g = bind(f, 1)`, `h = bind(g, 2)`). This goes one level further —
    // three chained `bind` calls over a `ref`-bound three-param target,
    // each currying one more `val` param, until the bound row exactly
    // matches the declared arity and the final call supplies zero
    // additional arguments (`h()`). Proves `bind`'s "consume the head of
    // the remaining param row" rule composes cleanly across an arbitrary
    // chain depth, not just two links.
    assert_case("fn-value-bind-triple-chain");
}

// ── T1c-2 (#700): gradual-mode dispatch faults (spec §3) ────────────────

/// Run a brink program to completion, returning the first runtime error (if
/// any) rather than panicking — for the turn-terminating fault cases.
fn run_expecting_fault(source: &str) -> Option<brink_runtime::RuntimeError> {
    let (program, tables) = compile_and_link(source);
    let mut story = Story::<DotNetRng>::new(program, tables);
    for _ in 0..64 {
        match story.continue_single() {
            Ok(Line::Done { .. } | Line::End { .. } | Line::Suspended { .. }) => return None,
            Ok(_) => {}
            Err(e) => return Some(e),
        }
    }
    None
}

#[test]
fn calling_a_non_function_value_is_a_turn_terminating_fault() {
    // Gradual mode: `x(3)` where `x` holds an int is a dispatch fault — no
    // silent garbage (spec §3, value-model §11c).
    let src = "~ temp x = 5\n~ temp y = x(3)\nUnreachable {y}.\n-> END\n";
    let err = run_expecting_fault(src).expect("calling an int must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotCallable(_)),
        "expected NotCallable, got {err:?}"
    );
}

#[test]
fn explicit_call_with_wrong_arity_is_a_turn_terminating_fault() {
    // The explicit `call(f, args…)` form carries argc, so a gradual-mode arity
    // mismatch faults exactly (spec §3/§4).
    let src = "~ temp d = #fn(double)\n~ temp r = call(d, 1, 2)\nUnreachable {r}.\n-> END\n\n\
               === function double(x) ===\n~ return x + x\n";
    let err = run_expecting_fault_gradual(src).expect("wrong arity must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::FunctionValueArity { .. }),
        "expected FunctionValueArity, got {err:?}"
    );
}

#[test]
fn bind_over_binding_more_than_remaining_params_is_a_turn_terminating_fault() {
    // `bind(f, …)` that supplies more args than the target has remaining
    // params is a turn-terminating fault (spec §3) — never a silently
    // truncated bound row. `double` takes one param, so binding two faults.
    // NS-A9: gradual explicitly — strict rejects over-binding statically.
    let src = "~ temp d = #fn(double)\n~ temp g = bind(d, 1, 2)\nUnreachable {g}.\n-> END\n\n\
               === function double(x) ===\n~ return x + x\n";
    let err = run_expecting_fault_gradual(src).expect("over-binding must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::FunctionValueArity { .. }),
        "expected FunctionValueArity, got {err:?}"
    );
}

// ── #721: direct-call `f(args…)` arity mismatch must fault cleanly ──────
//
// `f(args…)` compiles through the same `CallVariable` opcode as the classic
// divert-target-variable-call path (`{s(P)}` etc. — `run_story.rs`'s
// `function_variable_call`). Before #721, the popped-arg count for the
// function-value arm was *derived* from the resolved target's arity instead
// of carried as an `argc` operand, so a wrong-arity direct call could leave
// a stray value on the stack instead of faulting. These prove both
// directions fault via `FunctionValueArity`, matching the explicit
// `call(f, args…)` form above — never silent stack corruption.

#[test]
fn direct_call_with_too_many_args_is_a_turn_terminating_fault() {
    // `double` takes 1 param; the direct call supplies 2.
    let src = "~ temp d = #fn(double)\n~ temp r = d(1, 2)\nUnreachable {r}.\n-> END\n\n\
               === function double(x) ===\n~ return x + x\n";
    let err = run_expecting_fault_gradual(src).expect("wrong arity must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::FunctionValueArity { .. }),
        "expected FunctionValueArity, got {err:?}"
    );
}

#[test]
fn binding_a_non_function_value_is_a_turn_terminating_fault() {
    // `bind(x, …)` where `x` is not a function value is a dispatch fault
    // (spec §3) — same NotCallable posture as calling a non-function.
    let src = "~ temp x = 5\n~ temp g = bind(x, 1)\nUnreachable {g}.\n-> END\n";
    let err = run_expecting_fault(src).expect("binding a non-function must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotCallable(_)),
        "expected NotCallable, got {err:?}"
    );
}

#[test]
fn direct_call_with_too_few_args_is_a_turn_terminating_fault() {
    // `add` takes 2 params; the direct call supplies 1.
    let src = "~ temp d = #fn(add)\n~ temp r = d(1)\nUnreachable {r}.\n-> END\n\n\
               === function add(x, y) ===\n~ return x + y\n";
    let err = run_expecting_fault_gradual(src).expect("wrong arity must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::FunctionValueArity { .. }),
        "expected FunctionValueArity, got {err:?}"
    );
}

// ── T1c-4 (#702): mechanical tail — the remaining fault categories ───────
//
// docs/t1c-spec.md §3: "calling a non-function value, or calling with wrong
// arity or a wrong-typed argument, is a turn-terminating runtime fault" plus
// the cross-flow ref-`#@local` fault. Non-fn-callee and arity are covered
// above (T1c-2/T1c-3); these two round out the fault-category checklist.

#[test]
fn calling_through_a_fn_value_with_a_wrong_typed_argument_is_a_turn_terminating_fault() {
    // Gradual mode has no fn-value-specific static arg-type check at the
    // call boundary (spec §3/§4 — that's strict mode's job, and strict-mode
    // call/bind checking is explicitly out of this pass's scope, #733) — a
    // wrong-typed argument flows into the body and faults there exactly like
    // an ordinary direct call would. `square`'s `x * x` faults on a string
    // argument (`string_op` only defines `Add`/`Equal`/`NotEqual`).
    let src = "~ temp d = #fn(square)\n~ temp r = d(\"nope\")\nUnreachable {r}.\n-> END\n\n\
               === function square(x) ===\n~ return x * x\n";
    let err = run_expecting_fault_gradual(src).expect("wrong-typed argument must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::TypeError(_)),
        "expected TypeError, got {err:?}"
    );
}

#[test]
fn invoking_a_closure_that_ref_binds_a_local_cell_is_a_cross_flow_fault() {
    // spec §3/#597: a closure that `ref`-binds a `#@local` flow-private cell
    // is a turn-terminating fault on invocation — T1c ships this instead of
    // tracking creating-flow identity. `prepare_fn_value_call`'s guard reads
    // the bound cell's compiled scope bit unconditionally (no creating-flow
    // identity exists yet to compare against), so a single-flow story
    // exercises the fault directly — no second flow needed.
    let src = "#@local\nVAR mood = 10\n\n\
               ~ temp healer = #fn(heal, mood)\n~ temp r = healer(5)\nUnreachable {r}.\n-> END\n\n\
               === function heal(ref hp: int, amount: int): int ===\n~ hp = hp + amount\n~ return hp\n";
    let err = run_expecting_fault(src).expect("ref-binding a #@local cell must fault on invoke");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::FunctionValueCrossFlowLocal(_)
        ),
        "expected FunctionValueCrossFlowLocal, got {err:?}"
    );
}

#[test]
fn ordering_two_function_values_is_a_turn_terminating_fault() {
    // Structural `==`/`!=` are defined (spec §5), but ordering (`<`, `>`, …)
    // is not — a runtime fault in gradual mode (a compile error in strict).
    let src = "~ temp d = #fn(double)\n~ temp lt = d < d\nUnreachable {lt}.\n-> END\n\n\
               === function double(x) ===\n~ return x + x\n";
    let err = run_expecting_fault_gradual(src).expect("ordering fn values must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::TypeError(_)),
        "expected TypeError, got {err:?}"
    );
}

#[test]
fn direct_call_with_correct_arity_still_works() {
    // Regression guard: the argc-carrying dispatch must not disturb the
    // matching-arity happy path.
    let src = "~ temp d = #fn(double)\n~ temp r = d(21)\n{r}.\n-> END\n\n\
               === function double(x: int): int ===\n~ return x + x\n";
    let (program, tables) = compile_and_link(src);
    let mut story = Story::<DotNetRng>::new(program, tables);
    let out = run_to_end(&mut story);
    assert_eq!(out, "42.\n");
}

// ── Stdlib slice 1 completion: char_at (docs/t1b-surface-spec.md §5,
// issue #857) ──────────────────────────────────────────────────────────────

#[test]
fn char_at_out_of_range_index_is_a_turn_terminating_fault() {
    // §11c's "no silent garbage" default — an out-of-range index faults
    // rather than clamping or returning an empty string.
    let src = "~ temp c = char_at(\"hi\", 5)\nUnreachable {c}.\n-> END\n";
    let err = run_expecting_fault(src).expect("out-of-range char_at must fault");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::CharAtOutOfBounds { index: 5, len: 2 }
        ),
        "expected CharAtOutOfBounds, got {err:?}"
    );
}

#[test]
fn char_at_negative_index_is_a_turn_terminating_fault() {
    let src = "~ temp c = char_at(\"hi\", -1)\nUnreachable {c}.\n-> END\n";
    let err = run_expecting_fault(src).expect("negative char_at index must fault");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::CharAtOutOfBounds { index: -1, len: 2 }
        ),
        "expected CharAtOutOfBounds, got {err:?}"
    );
}

#[test]
fn char_at_on_a_non_string_is_a_turn_terminating_fault() {
    let src = "~ temp c = char_at(5, 0)\nUnreachable {c}.\n-> END\n";
    let err = run_expecting_fault(src).expect("char_at on a non-string must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::NotIndexable("int")),
        "expected NotIndexable, got {err:?}"
    );
}

#[test]
fn char_at_with_a_non_int_index_is_a_turn_terminating_fault() {
    let src = "~ temp c = char_at(\"hi\", \"x\")\nUnreachable {c}.\n-> END\n";
    let err = run_expecting_fault(src).expect("non-int char_at index must fault");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::CharAtIndexNotInt("string")
        ),
        "expected CharAtIndexNotInt, got {err:?}"
    );
}

#[test]
fn char_at_author_shadow_wins_over_the_builtin() {
    // §5's shadowing ruling: an author-defined `char_at` function resolves
    // normally instead of dispatching to the VM-native intrinsic.
    let src = "~ temp r = char_at(1, 2)\n{r}\n-> END\n\n\
               === function char_at(a: int, b: int): int ===\n~ return a + b\n";
    let (program, tables) = compile_and_link(src);
    let mut story = Story::<DotNetRng>::new(program, tables);
    let out = run_to_end(&mut story);
    assert_eq!(out, "3\n");
}

// ── T1c-2 (#700): persistence + rehydration (spec §6) ───────────────────

/// Compile `source` (brink dialect) and link it to a runnable program.
fn compile_and_link(
    source: &str,
) -> (
    std::sync::Arc<brink_runtime::Program>,
    Vec<Vec<brink_format::LineEntry>>,
) {
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    let output = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        },
    )
    .expect("compile");
    let (program, tables) = brink_runtime::link(&output.data).expect("link");
    (std::sync::Arc::new(program), tables)
}

/// Drain a story to its terminal line, concatenating text.
fn run_to_end(story: &mut Story<DotNetRng>) -> String {
    let mut out = String::new();
    loop {
        match story.continue_single().expect("runtime error") {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. }
            | Line::End { text, .. }
            | Line::Choices { text, .. }
            | Line::Suspended { text, .. } => {
                out.push_str(&text);
                break;
            }
        }
    }
    out
}

// A program that stores a function value in a global at `setup`, then invokes
// it from a separate `invoke` knot — so a save taken after `setup` carries a
// live fn value, and `invoke` can be reached after a load without re-running
// creation.
//
// NS-A9: the whole save/load quartet (scalar, array, map, rehydration)
// compiles under explicit gradual — the `VAR stored = 0` reassignment idiom
// gives `stored` a static int type under strict, making `stored(…)` E063.
// The subject here is VM save/load of fn values, which is regime-independent
// behavior; gradual is just the fixtures' native idiom.
const SAVE_LOAD_SRC: &str = "\
=== function double(x) ===
~ return x + x

VAR stored = 0

=== setup ===
~ stored = #fn(double)
Set up.
-> DONE

=== invoke ===
~ temp r = stored(21)
Result {r}.
-> END
";

#[test]
fn fn_value_save_load_invoke_equals_direct_invoke() {
    let (program, tables) = compile_and_link_gradual(SAVE_LOAD_SRC);

    // Direct: setup, then invoke — no save/load in between.
    let mut direct = Story::<DotNetRng>::new(std::sync::Arc::clone(&program), tables.clone());
    direct.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut direct);
    direct.choose_path_string("invoke").expect("goto invoke");
    let direct_out = run_to_end(&mut direct);

    // Save/load: setup, capture the game state (global holds the fn value),
    // load it into a fresh story, then invoke without re-running setup.
    let mut src = Story::<DotNetRng>::new(std::sync::Arc::clone(&program), tables.clone());
    src.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut src);
    let saved = src.save_state();

    let mut loaded = Story::<DotNetRng>::new(program, tables);
    let report = loaded.load_state(&saved);
    assert!(
        report.is_clean(),
        "save round-trip should reconcile cleanly: {report:?}"
    );
    loaded.choose_path_string("invoke").expect("goto invoke");
    let loaded_out = run_to_end(&mut loaded);

    assert_eq!(
        direct_out, loaded_out,
        "save→load→invoke must equal direct invoke",
    );
    assert_eq!(loaded_out, "Result 42.\n");
}

#[test]
fn fn_value_rehydration_faults_on_param_rename_and_remode() {
    // v1: `heal(ref hp, amount)` — the closure ref-binds a World cell.
    let v1 = "\
=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp

VAR world_hp = 10
VAR stored = 0

=== setup ===
~ stored = #fn(heal, world_hp)
Set up.
-> DONE
";
    // v2: same knot name (same fn token / DefinitionId) but the first param is
    // renamed AND re-moded (ref → val) — a saved closure must fault on invoke,
    // never silently misbind (spec §6).
    let v2 = "\
=== function heal(dose, amount) ===
~ return dose + amount

VAR world_hp = 10
VAR stored = 0

=== invoke ===
~ temp r = stored(5)
Result {r}.
-> END
";

    let (p1, t1) = compile_and_link_gradual(v1);
    let mut s1 = Story::<DotNetRng>::new(p1, t1);
    s1.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut s1);
    let saved = s1.save_state();

    let (p2, t2) = compile_and_link_gradual(v2);
    let mut s2 = Story::<DotNetRng>::new(p2, t2);
    let _ = s2.load_state(&saved);
    s2.choose_path_string("invoke").expect("goto invoke");

    // Invoking the rehydrated closure must be a defined fault.
    let mut err = None;
    for _ in 0..8 {
        match s2.continue_single() {
            Ok(Line::Done { .. } | Line::End { .. } | Line::Suspended { .. }) => break,
            Ok(_) => {}
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    let err = err.expect("rehydrated closure with a renamed/re-moded param must fault on invoke");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::FunctionValueRehydrationMismatch(_)
        ),
        "expected FunctionValueRehydrationMismatch, got {err:?}"
    );
}

// ── T1c-4 (#702): mechanical tail — save/load with fn values *inside* a
// collection ────────────────────────────────────────────────────────────
//
// `fn_value_save_load_invoke_equals_direct_invoke` above proves a fn value
// held directly in a scalar global survives save/load. Spec §6 makes no
// carve-out by container — "function values save like every other value" —
// so this proves the same roundtrip for a fn value sitting *inside* an
// array and a map, matching the collections' own existing COW/value-copy
// save/load coverage (`Value::Array`/`Value::Map` have no special-cased
// serialization path; a `Value::Closure` element rides along like any other
// element).

/// Same shape as `SAVE_LOAD_SRC` but the global holds an *array* of two fn
/// values (`#[#fn(double), #fn(double)]`) rather than a bare fn value —
/// `invoke` reads one element into a `temp` (a plain array index, not a
/// chained `arr[i](args)` call expression) and calls it from there.
const SAVE_LOAD_ARRAY_SRC: &str = "\
=== function double(x) ===
~ return x + x

VAR stored = 0

=== setup ===
~ stored = #[#fn(double), #fn(double)]
Set up.
-> DONE

=== invoke ===
~ temp f = stored[1]
~ temp r = f(21)
Result {r}.
-> END
";

#[test]
fn fn_value_inside_an_array_save_load_invoke_equals_direct_invoke() {
    let (program, tables) = compile_and_link_gradual(SAVE_LOAD_ARRAY_SRC);

    let mut direct = Story::<DotNetRng>::new(std::sync::Arc::clone(&program), tables.clone());
    direct.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut direct);
    direct.choose_path_string("invoke").expect("goto invoke");
    let direct_out = run_to_end(&mut direct);

    let mut src = Story::<DotNetRng>::new(std::sync::Arc::clone(&program), tables.clone());
    src.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut src);
    let saved = src.save_state();

    let mut loaded = Story::<DotNetRng>::new(program, tables);
    let report = loaded.load_state(&saved);
    assert!(
        report.is_clean(),
        "save round-trip should reconcile cleanly: {report:?}"
    );
    loaded.choose_path_string("invoke").expect("goto invoke");
    let loaded_out = run_to_end(&mut loaded);

    assert_eq!(
        direct_out, loaded_out,
        "save→load→invoke must equal direct invoke for a fn value inside an array",
    );
    assert_eq!(loaded_out, "Result 42.\n");
}

/// Same shape again, a *map* keyed by name instead of an array indexed by
/// position — proves the roundtrip isn't array-specific.
const SAVE_LOAD_MAP_SRC: &str = "\
=== function double(x) ===
~ return x + x

VAR stored = 0

=== setup ===
~ stored = #{\"double\": #fn(double)}
Set up.
-> DONE

=== invoke ===
~ temp f = stored[\"double\"]
~ temp r = f(21)
Result {r}.
-> END
";

#[test]
fn fn_value_inside_a_map_save_load_invoke_equals_direct_invoke() {
    let (program, tables) = compile_and_link_gradual(SAVE_LOAD_MAP_SRC);

    let mut direct = Story::<DotNetRng>::new(std::sync::Arc::clone(&program), tables.clone());
    direct.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut direct);
    direct.choose_path_string("invoke").expect("goto invoke");
    let direct_out = run_to_end(&mut direct);

    let mut src = Story::<DotNetRng>::new(std::sync::Arc::clone(&program), tables.clone());
    src.choose_path_string("setup").expect("goto setup");
    let _ = run_to_end(&mut src);
    let saved = src.save_state();

    let mut loaded = Story::<DotNetRng>::new(program, tables);
    let report = loaded.load_state(&saved);
    assert!(
        report.is_clean(),
        "save round-trip should reconcile cleanly: {report:?}"
    );
    loaded.choose_path_string("invoke").expect("goto invoke");
    let loaded_out = run_to_end(&mut loaded);

    assert_eq!(
        direct_out, loaded_out,
        "save→load→invoke must equal direct invoke for a fn value inside a map",
    );
    assert_eq!(loaded_out, "Result 42.\n");
}

// ── NS-A1: Option[T] + the ruled stdlib flips (issue #1107,
// docs/stdlib-spec.md §1.4 + §§3-5, docs/stdlib-sequencing.md §2 Wave A1) ──

/// End-to-end reachability for the whole A1 verb set: `find`/`index_of`/
/// `min`/`max`/`first`/`last`/`pop`/`get`/`contains_value`/`clear`, plus
/// `some(x)` construction, bare-`none` equality, and Option interpolation
/// — every `none`-valued interpolation slot here renders as nothing, the
/// §1.6b display-boundary forgiveness (Track B4, issue #1463; superseded
/// the interim total `none` render this fixture originally pinned).
#[test]
fn option_verbs_end_to_end() {
    assert_case("option-verbs");
}

/// NS-A8 (`docs/tower-mini-spec.md`, issue #1114): the numeric tower end to
/// end in the brink dialect — constructors (int lanes promote), the ruled
/// operator table (componentwise `+`/`-`/`*`, scalar scale both orders,
/// negation, `quat * vec` rotation, `mat * vec` transform), `dot`/`cross`,
/// the tower-wide two-arg `min`/`max` plus `clamp`/`lerp`, glam-named
/// component access (`a.x`, `m.y_axis`), componentwise-IEEE equality, and
/// the structural display default (`vec3 { x: …, … }`).
#[test]
fn numeric_tower_end_to_end() {
    assert_case("numeric-tower");
}

/// NS-A4 (`docs/stdlib-spec.md` §4b, issue #1110): the ordering verbs end
/// to end in the brink dialect — the F0 four-verb family
/// (`sort`/`sorted`/`sort_by`/`sorted_by`), the doctrine order over
/// ints/mixed numerics/strings/lexicographic arrays, stability, and the
/// re-entrant fn-value comparator. NaN-free data, so the default (dev)
/// mode and prod agree exactly — the modes-agree leg of the §4b gate; the
/// dev-fault / prod-order split is pinned by `brink-runtime`'s
/// `collection_ops` unit tests and this crate's `ns_a4_exec_mode`
/// `ExecMode` test.
#[test]
fn sort_verbs_end_to_end() {
    assert_case("sort-verbs");
}

/// NS-A7 (`docs/stdlib-spec.md` §8, issue #1113): `Weighted[T]` + the
/// humble heap end to end — construction + F17 multiset equality +
/// construction-literal display, seeded `roll` goldens (the rand-verbs
/// oracle-free discipline), and the heap verbs' push/peek/pop/drain over
/// an ordinary array (§4b doctrine order; an empty pop is `none`, which
/// renders as nothing at the interpolation boundary — §1.6b, Track B4,
/// issue #1463). Strict types (the brink-dialect default).
#[test]
fn weighted_heap_verbs_end_to_end() {
    assert_case("weighted-heap-verbs");
}

#[test]
fn author_defined_find_shadows_builtin_with_e035_warning() {
    // The A1 names ride the same shadowing machinery as `len` (§9.3's
    // E035-lineage posture).
    let source = "=== function find(s: string, x: string): int\n~ return 999\n\nHi {find(\"a\", \"b\")}.\n-> END\n";
    let out = compile_brink(source).expect("shadowing is a warning, not a compile error");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E035),
        "expected E035 shadow warning, got {:?}",
        out.warnings
    );
}

#[test]
fn author_defined_none_variable_shadows_the_literal_with_e035_warning() {
    let source = "VAR none = 5\nValue {none}.\n-> END\n";
    let out = compile_brink(source).expect("shadowing is a warning, not a compile error");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E035),
        "expected E035 shadow warning, got {:?}",
        out.warnings
    );
}

#[test]
fn clear_in_expression_position_is_a_compile_error() {
    // `clear` is a statement-only mutator (§5) — expression position is the
    // same E056 misuse as `push`/`insert`/`remove`.
    let source = "~ temp m = #{\"a\": 1}\n~ temp x = clear(m)\nNope {x}.\n-> END\n";
    let err = compile_brink(source).expect_err("clear as an expression must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E056),
        "expected E056, got {diags:?}"
    );
}

#[test]
fn clear_arity_mismatch_is_e058_with_the_signature() {
    let source = "~ {\n    clear()\n}\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("clear() with no args must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E058
                && d.message.contains("clear(map)")),
        "expected E058 naming clear(map), got {diags:?}"
    );
}

#[test]
fn clear_with_an_rvalue_argument_is_a_compile_error() {
    let source = "~ {\n    clear(#{\"a\": 1})\n}\nDone.\n-> END\n";
    let err = compile_brink(source).expect_err("rvalue clear argument must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E055),
        "expected E055, got {diags:?}"
    );
}

#[test]
fn pop_of_an_rvalue_is_a_compile_error() {
    // `pop(#[1, 2])` — mutating a temporary loses the mutation (the
    // mutation-posture ruling's rvalue-receiver error, free-call form).
    let source = "~ temp x = pop(#[1, 2])\nNope {x}.\n-> END\n";
    let err = compile_brink(source).expect_err("rvalue pop receiver must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E055),
        "expected E055, got {diags:?}"
    );
}

#[test]
fn pop_of_a_chained_lvalue_is_fenced_as_a_compile_error_for_a1() {
    // A1 scope fence: `pop(grid[0])` (a chained lvalue receiver) is not yet
    // lowerable through the expression-position RMW bracket — E055 with the
    // "bind it to a variable first" guidance rather than silent misbehavior.
    let source = "~ temp grid = #[#[1, 2]]\n~ temp x = pop(grid[0])\nNope {x}.\n-> END\n";
    let err = compile_brink(source).expect_err("chained pop receiver must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E055),
        "expected E055, got {diags:?}"
    );
}

// ── E107: bare `none` needs a type from context (§1.4) ───────────────────

#[test]
fn bare_none_var_declaration_is_e107() {
    let source = "VAR x = none\nHi.\n-> END\n";
    let err = compile_brink(source).expect_err("VAR x = none must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E107),
        "expected E107, got {diags:?}"
    );
}

#[test]
fn bare_none_const_declaration_is_e107() {
    let source = "CONST x = none\nHi.\n-> END\n";
    let err = compile_brink(source).expect_err("CONST x = none must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E107),
        "expected E107, got {diags:?}"
    );
}

#[test]
fn bare_none_temp_declaration_is_e107() {
    let source = "~ temp x = none\nHi.\n-> END\n";
    let err = compile_brink(source).expect_err("~ temp x = none must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E107),
        "expected E107, got {diags:?}"
    );
}

#[test]
fn bare_none_block_temp_declaration_is_e107() {
    let source = "~ {\n    temp x = none\n}\nHi.\n-> END\n";
    let err = compile_brink(source).expect_err("block temp x = none must be a compile error");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E107),
        "expected E107, got {diags:?}"
    );
}

#[test]
fn none_assignment_to_an_existing_slot_is_a_concrete_site_not_e107() {
    // §1.4: "concrete sites fine" — an assignment to an already-declared
    // slot has its type origin at the declaration; re-absencing it later is
    // exactly the intended use.
    let source = "~ temp r = some(1)\n~ r = none\n{r == none: absent now.}\n-> END\n";
    let out = compile_brink(source).expect("assignment-position none must compile");
    assert!(
        out.warnings
            .iter()
            .all(|w| w.code != brink_compiler::DiagnosticCode::E107),
        "no E107 expected, got {:?}",
        out.warnings
    );
}

#[test]
fn shadowed_none_declaration_is_not_e107() {
    // `none` resolving to a real user symbol (a LIST item here) is an
    // ordinary reference — E107 only fires for the *literal*.
    let source = "LIST mood = none, happy\nVAR m = none\nMood {m}.\n-> END\n";
    let out = compile_brink(source).expect("a resolvable `none` reference must compile");
    assert!(
        out.warnings
            .iter()
            .all(|w| w.code != brink_compiler::DiagnosticCode::E107),
        "no E107 expected for a shadowed none, got {:?}",
        out.warnings
    );
}

// ── Strict-ink rejection: the A1 surface is brink-dialect-gated ──────────

fn compile_strict_ink(
    source: &str,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        AnalysisOptions::default(), // dialect defaults to StrictInk
    )
}

#[test]
fn strict_ink_rejects_an_unresolved_option_verb_call() {
    let source = "~ temp x = find(\"a\", \"b\")\nNope {x}.\n-> END\n";
    let err = compile_strict_ink(source).expect_err("find under strict-ink must be E051");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E051),
        "expected E051, got {diags:?}"
    );
}

#[test]
fn strict_ink_rejects_a_bare_none_expression() {
    let source = "~ temp x = 1\n~ x = none\nNope {x}.\n-> END\n";
    let err = compile_strict_ink(source).expect_err("bare none under strict-ink must be E051");
    let diags = errors_of(&err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E051),
        "expected E051, got {diags:?}"
    );
}

// ── Runtime fault posture: malformed questions stay faults (§§3-5) ───────

#[test]
fn min_on_a_non_array_is_a_turn_terminating_fault() {
    let src = "~ temp x = min(5)\nUnreachable {x}.\n-> END\n";
    let err = run_expecting_fault(src).expect("min on an int must fault");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::StdlibWrongType { verb: "min", .. }
        ),
        "expected StdlibWrongType for min, got {err:?}"
    );
}

#[test]
fn min_over_mixed_type_elements_is_a_not_orderable_fault() {
    let src = "~ temp a = #[1, \"x\"]\n~ temp x = min(a)\nUnreachable {x}.\n-> END\n";
    let err = run_expecting_fault(src).expect("mixed-type min must fault");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::NotOrderable { verb: "min", .. }
        ),
        "expected NotOrderable for min, got {err:?}"
    );
}

/// NS-A3 boundary pin (issue #1109, docs/stdlib-spec.md §4b/§9.6): structs
/// have NO structural auto-order — ordering them requires an explicit
/// registered `compare` impl, and no impl spelling exists yet (⏳
/// code-dialect sitting), so `min`/`max` over an array of structs stays a
/// `NotOrderable` turn-terminating fault. Wave A4 wires registered
/// compares into the ordering verbs; this test is the line it moves.
#[test]
fn min_over_structs_without_compare_impl_is_not_orderable() {
    let src = "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n\
               ~ temp a = #[Point#{x: 1.0, y: 1.0}, Point#{x: 2.0, y: 2.0}]\n\
               ~ temp x = min(a)\nUnreachable {x}.\n-> END\n";
    let err = run_expecting_fault(src).expect("min over structs must fault");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::NotOrderable {
                verb: "min",
                found: "record",
            }
        ),
        "expected NotOrderable(record) for min, got {err:?}"
    );
}

#[test]
fn get_on_a_non_map_is_a_turn_terminating_fault() {
    let src = "~ temp a = #[1]\n~ temp x = get(a, 0)\nUnreachable {x}.\n-> END\n";
    let err = run_expecting_fault(src).expect("get on an array must fault");
    assert!(
        matches!(
            err,
            brink_runtime::RuntimeError::StdlibWrongType { verb: "get", .. }
        ),
        "expected StdlibWrongType for get, got {err:?}"
    );
}

#[test]
fn option_never_implicitly_unwraps_in_arithmetic() {
    // `some(1) + 1` — the ruled `Option[T] ≠ T` strictness at the runtime
    // (gradual-mode) backstop: a type fault, never a silent unwrap.
    let src = "~ temp x = some(1) + 1\nUnreachable {x}.\n-> END\n";
    let err = run_expecting_fault(src).expect("some(1) + 1 must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::TypeError(_)),
        "expected TypeError, got {err:?}"
    );
}

#[test]
fn pop_through_a_global_mutates_the_cell_and_yields_the_element() {
    let src = "VAR stack = 0\n~ stack = #[7, 8]\n~ temp x = pop(stack)\nPopped {x}, left {len(stack)}.\n-> END\n";
    let (program, tables) = compile_and_link(src);
    let mut story = Story::<DotNetRng>::new(program, tables);
    let out = run_to_end(&mut story);
    assert_eq!(out, "Popped some(8), left 1.\n");
}

// ── F27 (issue #1120): Option has no truthiness — the gradual-mode fault ──
//
// Ruled 2026-07-19 (docs/stdlib-spec.md §1.6), superseding NS-A1's shipped
// falsy-none: a condition-position `Option[T]` is `RuntimeError::
// OptionTruthiness` under gradual types (`types = strict` reports the same
// condition statically as E116 — see `tm3_strict_policy.rs`).

#[test]
fn option_in_conditional_guard_is_a_truthiness_fault() {
    // The exact `{r: …}` "did we find one?" guard A1 blessed — now a fault.
    let src = "~ temp r = find(\"ab\", \"b\")\n{r: found.}\n-> END\n";
    let err = run_expecting_fault(src).expect("option in condition must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::OptionTruthiness),
        "expected OptionTruthiness, got {err:?}"
    );
}

#[test]
fn none_in_conditional_guard_is_a_truthiness_fault_too() {
    // `none` is NOT falsy — absence has no truthiness either; the fault is
    // symmetric (no "presence is truthy / absence is falsy" halves).
    let src = "~ temp r = find(\"ab\", \"zz\")\n{r: found.|absent.}\n-> END\n";
    let err = run_expecting_fault(src).expect("none in condition must fault");
    assert!(
        matches!(err, brink_runtime::RuntimeError::OptionTruthiness),
        "expected OptionTruthiness, got {err:?}"
    );
}

#[test]
fn option_equality_guards_still_run_clean() {
    // The blessed spellings — `== none` / `== some(x)` — keep working; the
    // fault is scoped to truthiness coercion only.
    let src =
        "~ temp r = find(\"ab\", \"b\")\n{r == some(1): at one.}\n{r == none: absent.}\n-> END\n";
    let (program, tables) = compile_and_link(src);
    let mut story = Story::<DotNetRng>::new(program, tables);
    let out = run_to_end(&mut story);
    assert_eq!(out, "at one.\n");
}
