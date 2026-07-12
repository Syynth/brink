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
            Line::Done { text, .. } | Line::End { text, .. } => {
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
    let expected_msg = format!("read expected.txt for {name}");
    let expected = std::fs::read_to_string(dir.join("expected.txt")).expect(&expected_msg);
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
        "break-continue",
        "if-else-chain",
        "stdlib-len-and-contains",
        "stdlib-keys-and-values",
        "stdlib-push",
        "stdlib-insert",
        "stdlib-remove",
        "stdlib-mutator-nested-lvalue",
        "stdlib-shadowing",
        "deep-nesting-control-flow",
        "deep-nesting-collections",
        "weave-seam-two-knots",
        "weave-seam-stitch",
        "shadowing-triple-nested-blocks",
        "shadowing-for-loop-variable",
        "map-key-domain-contains-edges",
        "rmw-self-referential-flat-assignment",
        "rmw-chain-self-referential",
        "rmw-shared-map-cow",
        "rmw-mutator-shared-nested-lvalue",
    ];
    let mut found: Vec<String> = std::fs::read_dir(corpus_dir())
        .expect("read tests/tier1-brink")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
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

#[test]
fn author_defined_len_shadows_builtin_with_e035_warning() {
    let source = "=== function len(x)\n~ return 999\n\nHello.\n-> END\n";
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
            Line::Done { text, .. } | Line::End { text, .. } => {
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
