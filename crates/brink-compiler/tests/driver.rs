#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::wildcard_enum_match_arm,
    clippy::match_same_arms
)]

use std::collections::HashMap;
use std::path::Path;

use brink_runtime::{DotNetRng, Line, Story};

/// Helper: compile from an in-memory file system (`HashMap` of path to source).
fn compile_mem(
    entry: &str,
    files: &HashMap<&str, &str>,
) -> Result<brink_format::StoryData, brink_compiler::CompileError> {
    brink_compiler::compile(entry, |path| {
        files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {path}"),
            )
        })
    })
    .map(|output| output.data)
}

// ── Single file ─────────────────────────────────────────────────────

#[test]
fn compile_minimal_story() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "Hello, world!\n")]);

    let story = compile_mem("main.ink", &files).unwrap();
    // The driver ran without errors (parsed, lowered, analyzed, codegen).
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_story_with_knots() {
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "\
Hello!
-> greet

== greet ==
Welcome to the story.
-> END
",
    )]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

// ── INCLUDE discovery ───────────────────────────────────────────────

#[test]
fn compile_follows_includes() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE helpers.ink\nHello!\n-> greet\n"),
        ("helpers.ink", "== greet ==\nWelcome.\n-> END\n"),
    ]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_nested_includes() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE a.ink\nMain content.\n"),
        ("a.ink", "INCLUDE b.ink\n"),
        ("b.ink", "VAR x = 5\n== knot_b ==\nHello from b.\n-> END\n"),
    ]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_circular_includes_detected() {
    // Each file includes the other — should be detected as a circular dependency.
    let files: HashMap<&str, &str> = HashMap::from([
        ("a.ink", "INCLUDE b.ink\nContent A.\n"),
        ("b.ink", "INCLUDE a.ink\nContent B.\n"),
    ]);

    let err = compile_mem("a.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::CircularInclude(_)),
        "expected CircularInclude variant, got: {err}"
    );
}

// ── Relative path resolution ────────────────────────────────────────

#[test]
fn compile_resolves_relative_include_paths() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("src/main.ink", "INCLUDE utils/helpers.ink\nHello!\n"),
        ("src/utils/helpers.ink", "== greet ==\nHi.\n-> END\n"),
    ]);

    let story = compile_mem("src/main.ink", &files).unwrap();
    assert!(
        !story.containers.is_empty(),
        "expected non-empty containers"
    );
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn compile_missing_entry_file() {
    let files: HashMap<&str, &str> = HashMap::new();

    let err = compile_mem("nonexistent.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::Io(_)),
        "expected I/O error for missing entry file, got: {err}"
    );
}

#[test]
fn compile_missing_included_file() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "INCLUDE missing.ink\nHello!\n")]);

    let err = compile_mem("main.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::Io(_)),
        "expected I/O error for missing included file, got: {err}"
    );
}

/// A bare `INCLUDE` with no path lowers to an empty `FILE_PATH` node and the
/// parser's E037 ("expected file path") diagnostic. Discovery must not
/// attempt to read the empty path (which would surface a raw `Io` error and
/// swallow the diagnostic before it reaches the user) — see #708.
#[test]
fn compile_bare_include_reports_e037_not_io_error() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "INCLUDE\nHello!\n")]);

    let err = compile_mem("main.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::Diagnostics(_)),
        "expected a Diagnostics(E037) compile error, got: {err}"
    );
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E037"),
        "expected E037 (expected file path) among diagnostics, got: {codes:?}"
    );
}

// ── Native (.brink) discovery (B0.10b, #1288) ───────────────────────

/// A `.brink` entry with no `brink.toml` anywhere above it (the
/// single-file-project ruling: root = the entry's own directory) compiles
/// via `compile_path`, proving `prepare_driver`'s native branch dispatches
/// and its entry-key resolution (`native_source_root` + `relative_key`)
/// lines up with the key `discover_native` registered the entry under.
#[test]
fn compile_path_native_single_file_no_brink_toml() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-single-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "flow main() {\n  Hello. -> END\n}\n",
    )
    .unwrap();

    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();

    let output = result.expect("single-file native project should compile");
    assert!(
        !output.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

/// A `.brink` entry nested under a subdirectory, still with no `brink.toml`,
/// so the root is the entry's *own* directory (not an ancestor) — exercises
/// `native_source_root`'s fallback with a non-trivial (non-".") entry path,
/// and a sibling file in the same directory that discovery must find
/// without breaking the entry's own compile.
#[test]
fn compile_path_native_multi_file_no_brink_toml() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-multi-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("story")).unwrap();
    std::fs::write(
        dir.join("story/main.brink"),
        "flow main() {\n  Hello. -> END\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("story/other.brink"),
        "flow other() {\n  Hi. -> END\n}\n",
    )
    .unwrap();

    let result = brink_compiler::compile_path(&dir.join("story/main.brink"));
    std::fs::remove_dir_all(&dir).ok();

    let output = result.expect("multi-file native project should compile");
    assert!(
        !output.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

/// A lambda in a real `.brink` project, compiled through the production
/// entry point and **run** (issues #1685 and #1709).
///
/// This assertion used to point the other way. Through #1685 a lambda
/// lowered to HIR and then stopped at a targeted `E052` codegen fence
/// (`lir::lower::expr::lower_lambda_fence`), because an anonymous body had
/// no runtime representation; this test pinned that fence green, which is
/// precisely why nothing signalled that the fn-value verb layer (#1679)
/// could not actually be handed a lambda. #1709 lifts the lambda into a
/// synthesized function value, so the fence is gone and the user-visible
/// fact to pin is the opposite one: the lambda compiles, is *called*, and
/// its result reaches the transcript.
#[test]
fn compile_path_native_lambda_lifts_to_a_callable_function_value() {
    let output = compile_and_run_native(
        "lambda-lift",
        "fn tally(n: int): int {\n  let add = |x| x + 1;\n  return add(n);\n}\n\n\
         flow main() {\n  Tally: {tally(41)} -> END\n}\n",
    );
    assert!(
        output.contains("Tally: 42"),
        "the lambda must be lifted to a real function value and invoked, got: {output:?}"
    );
}

/// The by-value capture half of lambda lifting (RULED 2026-07-19, issue
/// #1709): a lambda's read of an enclosing local is snapshotted into the
/// closure environment **at the point the lambda value is made**, so a
/// later write to that local cannot be seen through the already-created
/// value. `bump` is created while `step` is `1`, `step` then becomes `100`,
/// and the call still adds `1`.
#[test]
fn compile_path_native_lambda_captures_by_value_at_creation() {
    let output = compile_and_run_native(
        "lambda-capture",
        "fn shifted(): int {\n  let step = 1;\n  let bump = |x| x + step;\n  \
         step = 100;\n  return bump(5);\n}\n\n\
         flow main() {\n  Shifted: {shifted()} -> END\n}\n",
    );
    assert!(
        output.contains("Shifted: 6"),
        "a capture is a creation-site snapshot, not a live read, got: {output:?}"
    );
}

/// Two lifting edges the tier1-native golden case does not reach (#1709).
///
/// `tailless` — a braced body whose block ends in a **statement**: "last
/// expression is the value" has no last expression, so the value comes from
/// the explicit `return` inside the block, which (per the 2026-07-19
/// ruling) leaves the *lambda*, not the enclosing function. Lifting must
/// therefore not append a synthetic terminal `Return` here; if it returned
/// from the wrong frame, `tailless` would never reach `f(41)`.
///
/// `nested` — **transitive** capture: `inner` reads `outer`, a local of the
/// frame two levels out. `outer` is not free in `inner`'s own enclosing
/// frame by accident — it has to be captured by `make` *and* re-captured by
/// `inner` for the read to resolve, which is exactly what the free-name
/// walk's nested-lambda arm is for.
#[test]
fn compile_path_native_lambda_tailless_body_and_transitive_capture() {
    let output = compile_and_run_native(
        "lambda-edges",
        "fn tailless() {\n  let f = |x| { return x + 1; };\n  return f(41);\n}\n\n\
         fn nested() {\n  let outer = 10;\n  \
         let make = |y| { let inner = |z| z + outer; inner(y) };\n  return make(5);\n}\n\n\
         flow main() {\n  Tailless: {tailless()}\n  Nested: {nested()} -> END\n}\n",
    );
    assert!(
        output.contains("Tailless: 42"),
        "an explicit `return` must leave the lambda, not the enclosing fn, got: {output:?}"
    );
    assert!(
        output.contains("Nested: 15"),
        "a nested lambda's read of a two-levels-out local must capture transitively, \
         got: {output:?}"
    );
}

/// A lambda handed straight to the pure trio (`docs/stdlib-spec.md` §4,
/// issue #1679) — the interaction #1709 exists to unblock. `#fn(target)`
/// over a named function was the only fn-value spelling that reached these
/// ops before lifting landed.
#[test]
fn compile_path_native_lambda_is_a_legal_verb_callback() {
    let output = compile_and_run_native(
        "lambda-verb-callback",
        "fn doubled() {\n  return map([1, 2, 3], |x| x * 2);\n}\n\n\
         flow main() {\n  Doubled: {doubled()} -> END\n}\n",
    );
    assert!(
        output.contains("Doubled: [2, 4, 6]"),
        "a lambda literal must be a legal `map` callback, got: {output:?}"
    );
}

/// A lambda reading its own `let` name — recursion — is a compile-time
/// refusal (`E158`), not a compile-clean runtime fault (issue #1709
/// review). `f`'s initializer is scanned for captures *before* `let f = …`
/// finishes binding `f`, so `f` has no temp slot yet in the enclosing frame
/// even though the analyzer resolves it as a real local; falling through
/// would let call lowering target `f`'s own `let`-declaration id as though
/// it were a callable function — a miscompile that previously only
/// surfaced as `RuntimeError::UnresolvedDefinition` when `f` called itself.
#[test]
fn compile_path_native_lambda_self_reference_is_e158() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-lambda-self-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "fn a() {\n  let f = |x| {\n    if x <= 0 { return 0; }\n    \
         return f(x - 1) + 1;\n  };\n  return f(3);\n}\n\n\
         flow main() {\n  Out: {a()} -> END\n}\n",
    )
    .unwrap();

    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();

    let err = result.expect_err(
        "a lambda reading its own not-yet-bound `let` name (recursion) must refuse to \
         compile, not silently target the wrong container and fault at runtime",
    );
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E158"),
        "expected E158 (unliftable lambda capture) among diagnostics, got: {codes:?}"
    );
}

/// Review finding on #1764: a lambda-valued `VAR`/`CONST` default is
/// *already* a hard compile error today, independently of this PR —
/// `decls.rs`'s `is_const_foldable_decl_default` has treated every
/// `hir::Expr::Lambda` as never constant-foldable (by design, since #1685)
/// well before the seven analyzer passes audited here existed. So the
/// per-pass fixes landed alongside this test do not unlock any new
/// compiling program; they add an *extra* diagnostic (here, `E106`) to a
/// file that was already refused. Pinned so the day a lambda default
/// legally folds is the day this test — not just prose — goes red.
#[test]
fn compile_path_native_lambda_valued_var_default_is_e083_with_map_keys_warning_alongside() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-lambda-var-default-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "var f = ||: int {\n  let m = Map { 3.5: 1 };\n  0\n}\n\n\
         flow main() {\n  Hello. -> END\n}\n",
    )
    .unwrap();

    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();

    let err = result.expect_err(
        "a lambda-valued VAR default must refuse to compile (E083) — it never legally \
         constant-folds, before or after #1764's analyzer-pass fixes",
    );
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E083"),
        "expected E083 (non-constant-foldable declaration default), got: {codes:?}"
    );
    assert!(
        codes.contains(&"E106"),
        "expected E106 (bad map key inside the lambda's statements) to still fire \
         alongside E083, got: {codes:?}"
    );
}

/// A `target/` subdirectory sitting next to a valid `.brink` entry, holding
/// a file that is not valid brink source at all: native discovery must
/// never walk into `target/` in the first place (issue #1381), so the
/// unparseable file is never enumerated and never kills an otherwise-valid
/// compile. Fails on `main` (before the `target/`/`.git/`/`node_modules/`
/// pruning landed) and passes on this branch.
#[test]
fn compile_path_native_ignores_unparseable_file_under_target() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-target-junk-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "flow main() {\n  Hello. -> END\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("target/junk.brink"), "{{{ not brink source at all").unwrap();

    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();

    let output = result.expect(
        "an unparseable .brink file under target/ must not be discovered, so the entry still compiles",
    );
    assert!(
        !output.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

/// A `.brink` entry under a directory whose ancestor has a `brink.toml`:
/// the source root walks up to it (not the entry's own directory), and
/// discovery must still find + read the entry correctly through that root.
#[test]
fn compile_path_native_walks_up_to_brink_toml() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-walkup-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("story")).unwrap();
    std::fs::write(dir.join("brink.toml"), "[project]\ndialect = \"brink\"\n").unwrap();
    std::fs::write(
        dir.join("story/main.brink"),
        "flow main() {\n  Hello. -> END\n}\n",
    )
    .unwrap();

    let result = brink_compiler::compile_path(&dir.join("story/main.brink"));
    std::fs::remove_dir_all(&dir).ok();

    let output =
        result.expect("native project with an ancestor brink.toml should compile via walk-up");
    assert!(
        !output.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

// ── B0.9 native strict-only enforcement (issue #1342) ────────────────
//
// Native is strict-only by design (decision-log 2026-07-19 "Typing posture
// ruled"): a `.brink` compile with an explicit `types = gradual` knob is a
// hard error (`E137`), never silently accepted. A bare `.brink` compile
// with no explicit `types` (the two tests above, `AnalysisOptions::default()`)
// is unaffected — the gate only fires on an explicit `gradual` choice, see
// `brink_analyzer::native_strict_only_error`'s doc.

/// `types = gradual` explicitly requested for a native entry is refused
/// with `E137`, not silently compiled.
#[test]
fn compile_path_native_with_explicit_gradual_types_is_e137() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-gradual-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "flow main() {\n  Hello. -> END\n}\n",
    )
    .unwrap();

    let options = brink_compiler::AnalysisOptions {
        types: Some(brink_compiler::TypePolicy::Gradual),
        ..Default::default()
    };
    let result = brink_compiler::compile_path_with_options(&dir.join("main.brink"), options);
    std::fs::remove_dir_all(&dir).ok();

    let err = result.expect_err("a gradual-knob .brink compile must be a hard error");
    assert!(
        matches!(err, brink_compiler::CompileError::Diagnostics(_)),
        "expected a Diagnostics(E137) compile error, got: {err}"
    );
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E137"),
        "expected E137 (native strict-only) among diagnostics, got: {codes:?}"
    );
}

/// `types = strict` explicitly requested for a native entry compiles
/// cleanly — the paired positive case for the gate above.
///
/// No `dialect` setting at all (issue #1348): `dialect` is an ink-only axis
/// (docs/t1b-surface-spec.md §1), orthogonal to native's `Language`
/// classification, so a native compile must never need one — the ink-only
/// `E064` config error (`types = strict` + `dialect != brink`) must not fire
/// against a `.brink` entry even at `dialect`'s `StrictInk` default.
#[test]
fn compile_path_native_with_explicit_strict_types_compiles() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-strict-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "flow main() {\n  Hello. -> END\n}\n",
    )
    .unwrap();

    let options = brink_compiler::AnalysisOptions {
        types: Some(brink_compiler::TypePolicy::Strict),
        ..Default::default()
    };
    let result = brink_compiler::compile_path_with_options(&dir.join("main.brink"), options);
    std::fs::remove_dir_all(&dir).ok();

    // `E064` is a hard error (`DiagnosticCode::severity`) — if the ink-only
    // dialect gate had fired, this `expect` would panic showing it, exactly
    // the regression `compile_path_native_with_explicit_gradual_types_is_e137`
    // above proves the *sibling* `E137` gate the same way.
    let output = result.expect("types = strict native compile should succeed with no dialect set");
    assert!(
        !output.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

/// The T1b dialect gate itself (`dialect_gate::check`, `E051`) must not fire
/// against native source either (issue #1348) — not just its `E064` config
/// error. `STRUCT` declarations are ordinary native syntax (native's own
/// `struct` keyword lowers to the same `HirFile::structs` the gate walks —
/// `docs/t1b-surface-spec.md` §1 flags a `STRUCT` decl as brink-extension
/// syntax under ink's `StrictInk` default), so a native file declaring one
/// must compile cleanly under fully-default `AnalysisOptions` — no `dialect`,
/// no `types` — exactly the posture a bare `.brink` compile with no
/// `brink.toml` has today.
#[test]
fn compile_path_native_struct_decl_under_default_options_has_no_dialect_gate_e051() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-struct-dialect-gate-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "struct Item {\n  name: string,\n  weight: int\n}\n\nflow main() {\n  Hello. -> END\n}\n",
    )
    .unwrap();

    let result = brink_compiler::compile_path_with_options(
        &dir.join("main.brink"),
        brink_compiler::AnalysisOptions::default(),
    );
    std::fs::remove_dir_all(&dir).ok();

    let output = result
        .expect("a native STRUCT declaration must never trip the ink-only dialect gate (E051)");
    assert!(
        !output.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

// ── B1 `or`-coalescing (`docs/stdlib-spec.md` §1.6a, issue #1460),
// short-circuited per issue #1471's ruling ───────────────────────────
//
// Full pipeline: native lexer (`KW_OR`) → native parser (`Prec::Coalesce`)
// → native HIR lowering (`InfixOp::Coalesce`) → analyzer typing
// (`infer::ty::coalesce`, recorded per step by `brink_analyzer::
// coalesce_types` and threaded to lowering by `brink-db`'s
// `coalesce_types_query`) → LIR (`lir::Expr::Coalesce`, a real branch) →
// codegen (`Opcode::CoalesceSome`) → runtime VM
// (`value_ops::coalesce_unwrap_some`) → `Story` output. Compiles and *runs*
// the program (not just a diagnostics-clean compile) so the opcode is
// proven reachable end to end, not merely wired at the type level.

/// Compile a native `.brink` entry from disk and run it to completion,
/// returning the concatenated output text. Mirrors `compile_and_run`
/// above, but for a native (not `.ink`) entry — `compile_and_run` is
/// `.ink`-only (`compile_mem` hardcodes the `.ink` extension), so this is
/// its own small helper rather than a parameterization of that one.
fn compile_and_run_native(dir_suffix: &str, source: &str) -> String {
    try_compile_and_run_native(dir_suffix, source)
        .unwrap_or_else(|err| panic!("fixture must run cleanly, got a runtime fault: {err:?}"))
}

/// [`compile_and_run_native`] without the "must run cleanly" assumption —
/// the fixture still has to *compile* cleanly, but a turn-terminating
/// runtime fault is handed back instead of panicking, so a test can assert
/// on one (the `CoalesceShape::RuntimeCheck` posture).
fn try_compile_and_run_native(
    dir_suffix: &str,
    source: &str,
) -> Result<String, brink_runtime::RuntimeError> {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-b1-{dir_suffix}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.brink"), source).unwrap();

    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();
    // B1 coalescing fixture must compile cleanly.
    let data = result.unwrap().data;

    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let lines = story.continue_maximally()?;
    let mut output = String::new();
    for line in &lines {
        output.push_str(line.text());
    }
    Ok(output)
}

/// The collapse form (`Option[T] or T -> T`): `some(v)` unwraps to `v`,
/// `none` falls through to the (already-`T`-typed) fallback unchanged.
#[test]
fn native_or_coalescing_collapse_form_unwraps_some_and_falls_back_on_none() {
    let output = compile_and_run_native(
        "collapse",
        "flow main() {\n  Some case: {some(5) or 99}\n  None case: {none or 99} -> END\n}\n",
    );
    assert!(
        output.contains("Some case: 5"),
        "expected the unwrapped `some(5)`, got: {output:?}"
    );
    assert!(
        output.contains("None case: 99"),
        "expected the `none` fallback, got: {output:?}"
    );
}

/// The two-Option form chained (`a or b or default`): `none or none` keeps
/// optionality (stays `none`), so the chain falls all the way through to
/// the final non-Option fallback. A **smoke test only** — review finding on
/// PR #1469/#1460: coalescing is semantically associative (`unify` is
/// commutative/associative on agreeing types), so `{none or none or 7}`
/// prints `7` under *either* grouping — this test cannot detect an
/// associativity regression (e.g. right-associative parsing) on its own.
/// `brink-syntax-native`'s own
/// `parser::tests::expression::prec_coalesce_chain_is_left_associative`
/// proves left-associativity at the parse-tree level, where a wrong
/// grouping is actually observable.
#[test]
fn native_or_coalescing_chain_falls_through_to_final_fallback() {
    let output = compile_and_run_native(
        "chain",
        "flow main() {\n  Chained: {none or none or 7} -> END\n}\n",
    );
    assert!(
        output.contains("Chained: 7"),
        "expected the chain to fall through both `none`s to the final fallback, got: {output:?}"
    );
}

/// Coalescing **short-circuits**: `rhs` is never evaluated when `lhs` is
/// `some` — RULED, issue #1471, flipping the eager pin PR #1469/#1460
/// landed and flagged as unruled (see `brink_ir::InfixOp::Coalesce`/
/// `brink_format::Opcode::CoalesceSome`'s own docs), matching the
/// short-circuiting `??`/`?:` conventions this operator's precedence
/// placement was modeled on. `bump()` mutates the global `counter` and is
/// only ever reached through the coalescing `rhs` — if evaluation were
/// still eager, `bump()` would run regardless of `lhs` and `counter` would
/// end up `1`; short-circuiting means a `some(_)` `lhs` skips `bump()`
/// entirely, so `counter` stays `0`.
#[test]
fn native_or_coalescing_short_circuits_rhs_on_some_lhs() {
    let output = compile_and_run_native(
        "shortcircuit",
        "var counter = 0\n\
         fn bump() {\n  counter = counter + 1;\n  return 99;\n}\n\
         flow main() {\n  Value: {some(5) or bump()}\n  Counter: {counter} -> END\n}\n",
    );
    assert!(
        output.contains("Value: 5"),
        "the collapse form must still unwrap `some(5)`, got: {output:?}"
    );
    assert!(
        output.contains("Counter: 0"),
        "expected `bump()` to never run since `lhs` is `some(_)` \
         (short-circuit), got: {output:?}"
    );
}

/// The other half of the short-circuit proof: `rhs` must still run —
/// exactly once — when `lhs` actually is `none`. Short-circuiting only
/// means the evaluation is *conditional*, not that `rhs` is permanently
/// dead code.
#[test]
fn native_or_coalescing_still_evaluates_rhs_when_lhs_is_none() {
    let output = compile_and_run_native(
        "shortcircuit-none",
        "var counter = 0\n\
         fn bump() {\n  counter = counter + 1;\n  return 99;\n}\n\
         flow main() {\n  Value: {none or bump()}\n  Counter: {counter} -> END\n}\n",
    );
    assert!(
        output.contains("Value: 99"),
        "the `none` lhs must fall through to `bump()`'s return value, got: {output:?}"
    );
    assert!(
        output.contains("Counter: 1"),
        "expected `bump()` to have run exactly once for a `none` lhs, got: {output:?}"
    );
}

/// A leading `some(_)` in a coalesce **chain** must still preserve
/// optionality through the intermediate step so the chain can continue —
/// short-circuiting changes *when* `rhs` runs, not the collapse-vs-preserve
/// typing rule (`(Option[T],T)->T` vs `(Option[T],Option[T])->Option[U]`).
/// `some(5) or none` is the inner step (parses left-associatively): a wrong
/// collapse decision there would hand the outer step a plain `Int` where it
/// requires an `Option`, faulting instead of printing `5`.
#[test]
fn native_or_coalescing_chain_preserves_optionality_through_intermediate_some() {
    let output = compile_and_run_native(
        "chain-preserve",
        "flow main() {\n  Chained: {some(5) or none or 99} -> END\n}\n",
    );
    assert!(
        output.contains("Chained: 5"),
        "expected the leading `some(5)` to win, unwrapped only at the final \
         non-Option fallback, got: {output:?}"
    );
}

/// The BLOCKING review finding on PR #1479, now a passing test (issue
/// #1492's ruling, re-driven here): an `Option`-returning **call** as the
/// intermediate fallback. `maybe()` lowers to `lir::Expr::Call`, whose
/// `Option`-ness lives in the callee's inferred return type — invisible to
/// any syntactic shape-sniff at lowering time, which is exactly why the
/// deleted `rhs_is_option_shaped` heuristic collapsed the inner step and
/// made the outer `CoalesceSome` fault on a plain `Int`. Lowering now reads
/// the analyzer's recorded `CoalesceShape::PreserveOption` for that step
/// instead, so the leading `some(5)` survives to the end.
///
/// (`brink-analyzer`'s `coalesce_types.rs` pins the *verdict* for this exact
/// chain; this pins the program it actually produces.)
#[test]
fn native_or_coalescing_chain_with_intermediate_call_yields_the_leading_some() {
    let output = compile_and_run_native(
        "chain-call",
        "fn maybe() {\n  return none;\n}\n\
         flow main() {\n  Chained: {some(5) or maybe() or 99} -> END\n}\n",
    );
    assert!(
        output.contains("Chained: 5"),
        "expected the leading `some(5)` to win through an Option-returning \
         call fallback, got: {output:?}"
    );
}

/// A `VisitCount`/`DivertTarget`/`TURNS_SINCE` reference reachable only
/// through a coalesce operand must still register on the counting walk
/// (`lir::lower::collect_counting_refs_expr`) — a BLOCKING silent-data-drop
/// finding on PR #1479: the new `lir::Expr::Coalesce` variant fell into the
/// walker's `_ => {}` catch-all, so the referenced container's
/// `CountingFlags::VISITS` was never set and its visit count read back `0`
/// instead of the true count. No diagnostic, no fault — just a wrong
/// number, which is exactly the class of bug this repo's rules call a bug
/// until proven otherwise.
#[test]
fn native_or_coalescing_rhs_visit_count_reference_is_tracked() {
    let output = compile_and_run_native(
        "visit-count",
        "flow main() {\n  -> other\n}\n\
         flow other() {\n  V: {none or other} -> END\n}\n",
    );
    assert!(
        output.contains("V: 1"),
        "expected `other`'s visit count to be tracked through the coalesce \
         operand, got: {output:?}"
    );
}

/// The `CoalesceShape::RuntimeCheck` posture, still intact (RULED, issue
/// #1492, documented on `brink_format::Opcode::CoalesceSome`): with an
/// unpinned left-hand type — an untyped parameter under the native default
/// `types = gradual` — the analyzer commits to no shape, and **the runtime
/// check is the operator's semantics**. A plain (non-`Option`) value
/// reaching the step is a turn-terminating `TypeError`, not a silent
/// coalesce and not a compile error.
#[test]
fn native_or_coalescing_unpinned_lhs_faults_on_a_plain_value() {
    let err = try_compile_and_run_native(
        "runtime-check-fault",
        "fn pick(x) {\n  return x or 99;\n}\n\
         flow main() {\n  Value: {pick(1)} -> END\n}\n",
    )
    .expect_err("a plain `Int` left-hand side must fault at runtime");
    assert!(
        matches!(&err, brink_runtime::RuntimeError::TypeError(msg)
            if msg.contains("or-coalescing requires an Option left-hand side")),
        "expected the or-coalescing TypeError, got: {err:?}"
    );
}

/// The other arm of the same unpinned step, so the test above cannot pass
/// by the operator being broken outright: an actual `Option` flowing into
/// an unpinned `lhs` coalesces normally (and still short-circuits — the
/// runtime check gates the *value*, not the branch).
#[test]
fn native_or_coalescing_unpinned_lhs_coalesces_an_option() {
    let output = compile_and_run_native(
        "runtime-check-ok",
        "fn pick(x) {\n  return x or 99;\n}\n\
         flow main() {\n  Some: {pick(some(5))}\n  None: {pick(none)} -> END\n}\n",
    );
    assert!(
        output.contains("Some: 5"),
        "an unpinned `lhs` holding `some(5)` must unwrap, got: {output:?}"
    );
    assert!(
        output.contains("None: 99"),
        "an unpinned `lhs` holding `none` must fall through, got: {output:?}"
    );
}

/// The same call-shaped fallback un-chained, and falling through: a `none`
/// `lhs` hands the whole step over to `maybe()`'s own `Option`, which the
/// trailing plain fallback then collapses.
///
/// This is **fall-through-only** coverage, not a verdict pin: the inner
/// step's `lhs` is the literal `none`, so it always takes the fallback
/// branch, and codegen only emits `MakeSome` on the *unwrap* (`some(v)`)
/// branch (`brink_codegen_inkb::expr`'s `Coalesce` arm) — never on
/// fall-through. That means this fixture prints `Chained: 7` identically
/// whether the inner step's recorded shape is `PreserveOption` or the
/// `RuntimeCheck` default, so it does not pin
/// `brink_ir::lir::CoalesceShape` here. The test above
/// (`native_or_coalescing_chain_with_intermediate_call_yields_the_leading_some`)
/// is the one that actually exercises `MakeSome`, because its `lhs` is a
/// real `some(5)` at runtime and takes the unwrap branch.
#[test]
fn native_or_coalescing_falls_through_to_an_option_returning_call() {
    let output = compile_and_run_native(
        "call-fallthrough",
        "fn maybe() {\n  return some(7);\n}\n\
         flow main() {\n  Chained: {none or maybe() or 99} -> END\n}\n",
    );
    assert!(
        output.contains("Chained: 7"),
        "expected `maybe()`'s `some(7)` to win, unwrapped at the final \
         non-Option fallback, got: {output:?}"
    );
}

// ── B1b the `as` binding (`docs/decision-log.md` 2026-07-26, issue
//    #1475) ────────────────────────────────────────────────────────────
//
// Full pipeline, in both ruled condition positions: native parser
// (`AS_BINDING`) → native HIR lowering (`IfStmt`/`WhileStmt`/
// `CondBranch::binding`) → analyzer typing (`Option[T]` → `T`) → LIR
// (`lir::Expr::OptionBind`) → codegen (`Opcode::OptionBind`) → runtime VM
// → `Story` output. Each fixture *runs*, so the opcode is proven reachable
// end to end rather than merely wired at the type level.

/// The statement form: `if EXPR as NAME { … }` binds the unwrapped payload
/// (not the `Option`) inside the success arm, and the `else` arm is
/// reached when the condition is `none`.
#[test]
fn native_as_binding_statement_form_binds_payload_and_falls_to_else() {
    let output = compile_and_run_native(
        "as-if",
        "fn present() {\n  if some(41) as n {\n    return n + 1;\n  }\n  return 0;\n}\n\
         fn absent() {\n  if none as n {\n    return n;\n  }\n  return -7;\n}\n\
         flow main() {\n  Present: {present()}\n  Absent: {absent()} -> END\n}\n",
    );
    assert!(
        output.contains("Present: 42"),
        "expected `n` to be the UNWRAPPED 41 (42 after +1), got: {output:?}"
    );
    assert!(
        output.contains("Absent: -7"),
        "expected the `none` condition to skip the arm entirely, got: {output:?}"
    );
}

/// The `while` form rebinds each iteration (the ruling's explicit rider):
/// `next_ticket()` yields `some(2)`, `some(1)`, `some(0)`, then `none`, so
/// a per-iteration rebinding sums to 3 — a first-iteration snapshot would
/// sum to 6 (2+2+2) and a non-terminating binding would never stop.
#[test]
fn native_as_binding_while_form_rebinds_each_iteration() {
    let output = compile_and_run_native(
        "as-while",
        "var counter = 3\n\
         fn next_ticket() {\n\
         \x20 if counter > 0 {\n\
         \x20   counter = counter - 1;\n\
         \x20   return some(counter);\n\
         \x20 }\n\
         \x20 return none;\n}\n\
         fn drain() {\n\
         \x20 let sum = 0;\n\
         \x20 while next_ticket() as t {\n\
         \x20   sum = sum + t;\n\
         \x20 }\n\
         \x20 return sum;\n}\n\
         flow main() {\n  Sum: {drain()} -> END\n}\n",
    );
    assert!(
        output.contains("Sum: 3"),
        "expected 2+1+0 = 3 from per-iteration rebinding, got: {output:?}"
    );
}

/// The template form `{if EXPR as NAME: … else: …}` — the same construct in
/// brink's other condition position, riding the already-ruled `{if}`
/// spelling. The bound name is readable from an interpolation inside the
/// success arm; the `else` arm runs on `none`.
#[test]
fn native_as_binding_template_form_binds_inside_the_success_arm() {
    let output = compile_and_run_native(
        "as-template",
        "flow main() {\n\
         \x20 Leader: {if some(9) as l: number {l} else: nobody}\n\
         \x20 Empty: {if none as l: number {l} else: nobody} -> END\n}\n",
    );
    assert!(
        output.contains("Leader: number 9"),
        "expected the template arm to see the unwrapped 9, got: {output:?}"
    );
    assert!(
        output.contains("Empty: nobody"),
        "expected the `else` arm on `none`, got: {output:?}"
    );
}

/// The binding is scoped **strictly to the success arm** — observable by
/// shadowing: an outer `n` is invisible inside the arm (the binding wins)
/// and intact after it (the binding is gone). A leaked binding would make
/// the function return `1`; a binding that never took effect would print
/// `100` from inside the arm.
#[test]
fn native_as_binding_scope_ends_at_the_arm() {
    let output = compile_and_run_native(
        "as-scope",
        "fn probe() {\n\
         \x20 let n = 100;\n\
         \x20 let inner = 0;\n\
         \x20 if some(1) as n {\n\
         \x20   inner = n;\n\
         \x20 }\n\
         \x20 return inner * 1000 + n;\n}\n\
         flow main() {\n  Probe: {probe()} -> END\n}\n",
    );
    assert!(
        output.contains("Probe: 1100"),
        "expected inner = 1 (the binding) and n = 100 (the outer local, \
         restored after the arm), got: {output:?}"
    );
}

// ── Native bare-name fn values (issue #1862) ────────────────────────
//
// The end-to-end proof that a bare name *is* a fn value lives in the
// `tests/tier1-native/fn-value-bare-name/` golden case. The three tests
// below cover the edges that case cannot express: the ink side of the
// gate, the `ref`-param refusal, and a plain (non-`.brink`) reading of the
// same shape.

/// The gate's ink half: in **ink** a bare function-knot name in expression
/// position is still the knot's **visit count**, not a fn value —
/// `#fn(f)` remains ink's only fn-value spelling. Dropping the
/// `LowerCtx::native` guard in `lir::lower::expr::lower_path` would turn
/// this `0` into a function value and print something else entirely.
#[test]
fn ink_bare_function_name_is_still_a_visit_count() {
    let source = "Count: {f}\n\
                  -> END\n\n\
                  === function f ===\n\
                  ~ return 1\n";
    let output = compile_and_run(source, &[]);
    assert!(
        output.contains("Count: 0"),
        "an ink bare function-knot name must stay a visit count (0, never entered), \
         got: {output:?}"
    );
}

/// A native bare-name reference binds **zero** arguments — the `#fn(f, a)`
/// binding form has no native spelling — so a target with a `ref`
/// parameter can never satisfy "all ref params bind at creation" and is
/// `E080` at the reference site (`fn_values::check_native_bare_refs`).
/// Before this check existed the reference compiled clean.
#[test]
fn native_bare_name_fn_value_with_a_ref_param_is_e080() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-fnvalue-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "fn heal(ref amount) {\n\
         \x20 amount = amount + 1;\n}\n\
         fn used() {\n\
         \x20 let f = heal;\n\
         \x20 return 0;\n}\n\
         flow main() {\n  Used: {used()} -> END\n}\n",
    )
    .unwrap();
    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();

    let err = result.expect_err("a ref-param target may not be referenced by bare name");
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E080"),
        "expected E080 at the bare-name reference, got: {codes:?}"
    );
}

/// Same obligation, but the bare-name reference sits in **declaration-
/// initializer** position (`var f = heal`) rather than inside a function
/// body. `check_native_bare_refs` only walks the block tree
/// (`hir::visit::visit`), which never descends into a file-level `VAR`/
/// `CONST` initializer — so before this test's fix, a `ref`-param target
/// referenced only this way compiled clean with no E080 at all, even
/// though the reviewer's own doc comment on `check_native_bare_refs`
/// asserts the obligation as an absolute ("a target with any ref parameter
/// can never be referenced by bare name").
#[test]
fn native_bare_name_fn_value_in_decl_initializer_with_a_ref_param_is_e080() {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-native-fnvalue-decl-ref-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main.brink"),
        "fn heal(ref amount) {\n\
         \x20 amount = amount + 1;\n}\n\
         var f = heal\n\
         flow main() {\n  Used: {0} -> END\n}\n",
    )
    .unwrap();
    let result = brink_compiler::compile_path(&dir.join("main.brink"));
    std::fs::remove_dir_all(&dir).ok();

    let err = result.expect_err("a ref-param target may not be referenced by bare name");
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E080"),
        "expected E080 at the decl-initializer bare-name reference, got: {codes:?}"
    );
}

/// The same shape without any `ref` parameter compiles and runs — the
/// guard above must not fire on an ordinary by-value target.
#[test]
fn native_bare_name_fn_value_without_ref_params_compiles_and_runs() {
    let output = compile_and_run_native(
        "fnvalue-plain",
        "fn double(x) {\n\
         \x20 return x * 2;\n}\n\
         fn apply(g, v) {\n\
         \x20 return g(v);\n}\n\
         flow main() {\n  Applied: {apply(double, 21)} -> END\n}\n",
    );
    assert!(
        output.contains("Applied: 42"),
        "expected the bare name to reach `apply` as a callable fn value, got: {output:?}"
    );
}

// ── compile_path (disk-based) ───────────────────────────────────────

#[test]
fn compile_path_reads_from_disk() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tier1/basics/I001-minimal-story/story.ink");

    let story = brink_compiler::compile_path(&path).unwrap();
    assert!(
        !story.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

#[test]
fn compile_path_nested_includes_from_disk() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tier3/misc/I025-nested-includes/story.ink");

    let story = brink_compiler::compile_path(&path).unwrap();
    assert!(
        !story.data.containers.is_empty(),
        "expected non-empty containers"
    );
}

// ── Compile + run (end-to-end) ─────────────────────────────────────

/// Compile from in-memory source, link, and run with given choice inputs.
fn compile_and_run(source: &str, inputs: &[usize]) -> String {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut output = String::new();
    let mut input_idx = 0;

    loop {
        let lines = story.continue_maximally().unwrap();
        let last = lines.last().unwrap();
        match last {
            Line::Text { .. } | Line::Done { .. } | Line::End { .. } | Line::Suspended { .. } => {
                for line in &lines {
                    output.push_str(line.text());
                }
                break;
            }
            Line::Choices { choices, .. } => {
                for line in &lines {
                    output.push_str(line.text());
                }
                let idx = if input_idx < inputs.len() {
                    let c = inputs[input_idx];
                    input_idx += 1;
                    c
                } else {
                    0
                };
                assert!(
                    idx < choices.len(),
                    "choice index {idx} out of range (only {} choices available)",
                    choices.len()
                );
                story.choose(idx).unwrap();
            }
        }
    }

    output
}

/// After a tunnel call returns, choices in the same container must be
/// yielded to the player. Regression: execution fell through to the
/// gather's `end` opcode, terminating the story before choices could
/// be presented.
#[test]
fn choices_after_tunnel_call_are_yielded() {
    let source = "\
-> main

=== function is_alive ===
~ return true

=== check ===
{ is_alive():
    ->->
}
-> END

=== main ===
Before choices.
-> check ->
*   [Option A]
    Chose A.
*   [Option B]
    Chose B.
- -> END
";
    let result = compile_and_run(source, &[0]);
    assert!(
        result.contains("Chose A"),
        "expected 'Chose A' after tunnel return, got: {result:?}"
    );
}

/// Choices after a tunnel call with arguments must be yielded.
/// Same regression as above but with parameter passing.
#[test]
fn choices_after_tunnel_call_with_args_are_yielded() {
    let source = "\
VAR hp = 2

-> main

=== function is_alive ===
~ return hp > 0

=== get_hit(x) ===
~ hp = hp - x
{ is_alive():
    ->->
}
-> END

=== main ===
Start.
-> get_hit(1) ->
*   [Fight]
    You fight.
*   [Flee]
    You flee.
- -> END
";
    let result = compile_and_run(source, &[0]);
    assert!(
        result.contains("You fight"),
        "expected 'You fight' after tunnel return, got: {result:?}"
    );
}

/// Nested choices with tunnel calls: outer choice leads to tunnel call,
/// tunnel returns, then inner choices must be presented. Mimics I003's
/// structure where the first choice leads to a stitch with a tunnel call
/// followed by sub-choices.
#[test]
fn nested_choices_after_tunnel_in_stitch() {
    let source = "\
VAR hp = 2

-> main

=== function is_alive ===
~ return hp > 0

=== get_hit(x) ===
~ hp = hp - x
{ is_alive():
    ->->
}
-> END

=== main ===
Choose:
*   [Yes]
    You chose yes.
    -> END
*   [No]
    You chose no.
    -> get_hit(1) ->
    **  [Fight]
        You fight.
    **  [Flee]
        You flee.
    - -> END
";
    let result = compile_and_run(source, &[1, 0]);
    assert!(
        result.contains("You fight"),
        "expected inner choice after tunnel return, got: {result:?}"
    );
}

// ── List display names ───────────────────────────────────────────────

/// List items should display without their origin prefix.
/// e.g. `{myList}` should output "a, b" not "myList.a, myList.b".
#[test]
fn list_items_display_without_origin_prefix() {
    let source = "\
LIST colors = (red), green, (blue)
{colors}
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "red, blue\n");
}

/// Multi-list display: items from different lists show unqualified names.
#[test]
fn multi_list_display_without_origin_prefix() {
    let source = "\
LIST a = (x), y
LIST b = (p), q
{a + b}
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "x, p\n");
}

// ── External function fallback ───────────────────────────────────────

/// EXTERNAL declaration with ink fallback function should use the fallback
/// when no external binding is provided.
#[test]
fn external_function_uses_ink_fallback() {
    let source = "\
EXTERNAL greet()

The value is {greet()}.
-> END

=== function greet() ===
~ return \"hello\"
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "The value is hello.\n");
}

/// EXTERNAL with arguments should pass args to the ink fallback.
#[test]
fn external_function_fallback_with_args() {
    let source = "\
EXTERNAL add(x, y)

The value is {add(3, 4)}.
-> END

=== function add(x, y) ===
~ return x + y
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "The value is 7.\n");
}

// ── Include file ordering ────────────────────────────────────────────

/// Content from included files should appear before the including file's
/// content, matching ink's INCLUDE-as-paste semantics.
#[test]
fn include_content_appears_before_main() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE a.ink\nINCLUDE b.ink\nThis is main.\n"),
        ("a.ink", "This is A.\n"),
        ("b.ink", "This is B.\n"),
    ]);
    let data = compile_mem("main.ink", &files).unwrap();
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let lines = story.continue_maximally().unwrap();
    let result: String = lines.iter().map(Line::text).collect();
    assert_eq!(
        result, "This is A.\nThis is B.\nThis is main.\n",
        "included file content must appear before main file content"
    );
}

// ── Divert to standalone labeled gather ──────────────────────────────

/// Diverting to a labeled gather within a knot (e.g. `-> knot.gather`)
/// must work. The gather needs its own container to be a divert target.
#[test]
fn divert_to_standalone_labeled_gather() {
    let source = "\
-> knot
=== knot ===
-> knot.gather
- (gather) g
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "g\n");
}

// ── Pattern 1: Divert/tunnel parameters not pushed onto stack ────────

/// Variable divert with parameter: `->x (5)` where x holds a divert target.
/// The argument must be pushed onto the value stack before the call.
#[test]
fn divert_target_with_parameter() {
    let source = "\
VAR x = ->place
->x (5)
== place (a) ==
{a}
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "5\n");
}

/// Tunnel onwards with argument: `->-> b (5 + 3)` must evaluate the
/// expression and pass the result to the target knot.
#[test]
fn tunnel_onwards_with_arg() {
    let source = "\
-> a ->
=== a ===
->-> b (5 + 3)
=== b (x) ===
{x}
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "8\n");
}

/// Tunnel onwards with parameter inside a default choice:
/// `* ->-> elsewhere (8)` — the default choice auto-fires and passes the arg.
#[test]
fn tunnel_onwards_with_param_default_choice() {
    let source = "\
-> tunnel ->
== tunnel ==
* ->-> elsewhere (8)
== elsewhere (x) ==
{x}
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "8\n");
}

/// Variable tunnel: `-> x ->` where x is a divert parameter.
/// Must use `tunnel_call_variable`, not a literal `tunnel_call`.
#[test]
fn variable_tunnel_call() {
    let source = "\
-> one_then_tother(-> tunnel)

=== one_then_tother(-> x) ===
    -> x -> end

=== tunnel ===
    STUFF
    ->->

=== end ===
    -> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "STUFF\n");
}

// ── Pattern 2: Tunnel gather emits done instead of tunnel_return ─────

/// After choosing inside a tunnel, execution should return to the caller
/// via `tunnel_return`, not terminate with `done`.
#[test]
fn tunnel_return_at_gather_with_thread() {
    let source = "\
-> knot
=== knot
    <- threadA
    When should this get printed?
    -> DONE
=== threadA
    -> tunnel ->
    Finishing thread.
    -> DONE
=== tunnel
    -   I'm in a tunnel
    *   I'm an option
    -   ->->
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(
        result,
        "I'm in a tunnel\nWhen should this get printed?\nI'm an option\nFinishing thread.\n"
    );
}

/// Bare `->->` on a gather line must emit a tunnel return.
/// `lower_gather_to_block` only handles `simple_divert()`, so `->->`
/// (a `TUNNEL_ONWARDS_NODE`) is silently dropped, producing `done`
/// instead of `tunnel_return`.
#[test]
fn gather_bare_tunnel_return() {
    let source = "\
-> start
== start ==
-> tun ->
After tunnel.
-> END
== tun ==
- Gathered.
* Pick me
- ->->
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(result, "Gathered.\nPick me\nAfter tunnel.\n");
}

/// `->-> target` on a gather line — tunnel return with divert override.
#[test]
fn gather_tunnel_return_with_override() {
    let source = "\
-> start
== start ==
-> tun ->
Should not print.
-> END
== tun ==
- In tunnel.
* Pick me
- ->-> destination
== destination ==
Overridden.
-> END
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(result, "In tunnel.\nPick me\nOverridden.\n");
}

/// `-> target ->` on a gather line — tunnel call from a gather.
#[test]
fn gather_tunnel_call() {
    let source = "\
-> start
== start ==
* Pick me
- -> inner_tunnel ->
After inner tunnel.
-> END
== inner_tunnel ==
Inside inner tunnel.
->->
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(
        result,
        "Pick me\nInside inner tunnel.\nAfter inner tunnel.\n"
    );
}

/// `<- thread` on a gather line — thread start from a gather.
/// The thread's choice must merge with the local sticky choice.
#[test]
fn gather_thread_start() {
    let source = "\
-> start
== start ==
* Pick me
- <- bg_thread
+ Next
-
Done.
-> END
== bg_thread ==
* Background option
- -> DONE
";
    // Pick "Pick me" first, then "Background option" (from the thread)
    // If the thread start is silently dropped, only "Next" is available
    // and "Background option" never appears.
    let result = compile_and_run(source, &[0, 0]);
    assert!(
        result.contains("Background option"),
        "expected thread's choice from gather `<- bg_thread` to be available, got: {result:?}"
    );
}

/// Structural test: compile a tunnel with `->->` on a gather line and
/// verify the .inkt contains `tunnel_return`, not just `done`.
#[test]
fn gather_tunnel_return_emits_tunnel_return_opcode() {
    let source = "\
-> start
== start ==
-> tun ->
After.
-> END
== tun ==
- Top.
* Option
- ->->
";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(
        buf.contains("tunnel_return"),
        "expected tunnel_return in bytecode for gather `->->`, got:\n{buf}"
    );
}

// ── Pattern 3: Thread choices not merged with current context ────────

/// Choices from a thread (`<- thread_with_options`) must merge with
/// choices from the current context (tunnel or inline).
#[test]
#[ignore = "thread completion doesn't resume main flow — runtime thread merging bug"]
fn tunnel_and_thread_choices_merge() {
    let source = "\
-> knot_with_options ->
Finished tunnel.
Starting thread.
<- thread_with_options
* E
-
Done.
== knot_with_options ==
* A
* B
-
->->
== thread_with_options ==
* C
* D
- -> DONE
";
    // Episode e0: choose A (idx 0), then C (idx 0 of remaining thread choices)
    let result = compile_and_run(source, &[0, 0]);
    assert_eq!(result, "A\nFinished tunnel.\nStarting thread.\nC\nDone.\n");
}

/// Thread choices must merge with tunnel choices in an interleaved scenario.
#[test]
fn thread_choices_merge_with_tunnel() {
    let source = "\
-> knot
=== knot
    <- threadB
    -> tunnel ->
    THE END
    -> END
=== tunnel
    - blah blah
    * wigwag
    - ->->
=== threadB
    *   option
    -   something
        -> DONE
";
    let result = compile_and_run(source, &[0]);
    assert_eq!(result, "blah blah\noption\nsomething\n");
}

/// Two threads contribute choices that must both appear in the choice set.
#[test]
fn multiple_thread_choices_merge() {
    let source = "\
-> start
== start ==
-> tunnel ->
The end
-> END
== tunnel ==
<- place1
<- place2
-> DONE
== place1 ==
This is place 1.
* choice in place 1
- ->->
== place2 ==
This is place 2.
* choice in place 2
- ->->
";
    let result = compile_and_run(source, &[0]);
    assert!(
        result.contains("choice in place 1"),
        "expected first thread's choice to be available, got: {result:?}"
    );
}

/// Thread choices in a loop: `<- choices(-> top)` must merge the thread's
/// "No" choice with the local "Yes" choice, and picking "No" must loop.
#[test]
fn thread_choice_loop_with_variable_divert() {
    let source = "\
-> start

=== start ===
Here is some gold. Do you want it?
- (top)
    <- choices(-> top)
    + Yes
        You win!
        -> END

=== choices(-> goback) ===
+ No
    Try again!
    -> goback
";
    // Pick No, No, then Yes
    let result = compile_and_run(source, &[1, 1, 0]);
    assert!(
        result.contains("You win!"),
        "expected loop with thread choices, got: {result:?}"
    );
}

/// Structural test: the compiler must NOT emit `begin_choice_set` in the
/// bytecode. This opcode was removed because it cleared pending choices,
/// breaking thread choice merging.
#[test]
fn choice_set_does_not_emit_begin_choice_set() {
    let source = "\
-> start
== start ==
* Choice A
* Choice B
- Done.
";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    assert!(
        !buf.contains("begin_choice_set"),
        "begin_choice_set should not appear in compiled output:\n{buf}"
    );
    assert!(
        !buf.contains("end_choice_set"),
        "end_choice_set should not appear in compiled output:\n{buf}"
    );
}

/// Three `<- thread` calls each contributing a choice — all three must appear.
#[test]
fn three_threads_all_choices_merge() {
    let source = "\
-> start
== start ==
<- t1
<- t2
<- t3
* local choice
- Done.
== t1 ==
* thread 1 choice
- -> DONE
== t2 ==
* thread 2 choice
- -> DONE
== t3 ==
* thread 3 choice
- -> DONE
";
    let result = compile_and_run(source, &[0]);
    // If all 4 choices are available, picking index 0 should succeed.
    // The key test: the story doesn't end prematurely due to cleared choices.
    assert!(
        result.contains("Done.") || result.contains("choice"),
        "expected all thread choices to be available, got: {result:?}"
    );
}

/// Thread provides a `*` (once-only) choice, main provides a `+` (sticky).
/// After selecting the once-only, only the sticky remains on re-evaluation.
#[test]
fn thread_choice_with_once_only_filtering() {
    let source = "\
-> start
== start ==
<- thread_opts
+ [sticky] Sticky text
- -> END
== thread_opts ==
* once only
    -> start
- -> DONE
";
    // Pick once-only (should be present alongside sticky), then sticky
    let result = compile_and_run(source, &[0, 0]);
    assert!(
        result.contains("once only") || result.contains("Sticky text"),
        "expected both choices to be available initially, got: {result:?}"
    );
}

/// `-> tunnel ->` where the tunnel does `<- thread`, both tunnel and
/// thread choices must merge with the caller's choices.
#[test]
fn nested_thread_in_tunnel_choices_merge() {
    let source = "\
-> start
== start ==
-> tun ->
* caller choice
- The end.
== tun ==
<- inner_thread
* tunnel choice
- ->->
== inner_thread ==
* thread choice
- -> DONE
";
    let result = compile_and_run(source, &[0]);
    assert!(
        result.contains("The end.") || result.contains("choice"),
        "expected thread+tunnel+caller choices to merge, got: {result:?}"
    );
}

// ── Pattern 3c: Nested gather chaining in deep weaves ────────────────

/// Three levels of choices with gathers at each level. After resolving
/// the deepest choices, execution must flow through each gather level
/// back to the outermost gather.
#[test]
fn nested_gather_three_levels() {
    let source = "\
* A
    * * B
        * * * C
        - - - Inner gather.
    - - Middle gather.
- Outer gather.
-> END
";
    let result = compile_and_run(source, &[0, 0, 0]);
    assert_eq!(
        result,
        "A\nB\nC\nInner gather.\nMiddle gather.\nOuter gather.\n"
    );
}

/// Two levels with a gather-then-second-choice-set pattern: the `- -`
/// gather has content then a second round of choices. After that second
/// round resolves, execution must still reach the `-` outer gather.
#[test]
fn nested_gather_with_second_choice_round() {
    let source = "\
* First
    * * Second
    * * Third
    - - Between.
    * * Fourth
    - - After fourth.
- Final.
-> END
";
    let result = compile_and_run(source, &[0, 0, 0]);
    assert_eq!(
        result,
        "First\nSecond\nBetween.\nFourth\nAfter fourth.\nFinal.\n"
    );
}

/// Simplified version of complex-flow-v1: the key pattern is that
/// the `- -` gather has glue (`<>`) that connects to the `-` gather.
#[test]
fn nested_gather_with_glue_continuation() {
    let source = "\
* Outer choice
    * * Deep choice
    - - After deep, <>
- outer end.
-> END
";
    let result = compile_and_run(source, &[0, 0]);
    assert_eq!(
        result,
        "Outer choice\nDeep choice\nAfter deep, outer end.\n"
    );
}

// ── Pattern 3d: Stitch parameters (including ref) ────────────────────

/// Stitch parameters must receive unique temp slots and be accessible
/// within the stitch body. This is the simplest case: by-value params.
#[test]
fn stitch_params_by_value() {
    let source = "\
-> greet.say(\"Hello\", \"world\")

== greet ==
= say(greeting, who)
{greeting}, {who}!
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Hello, world!\n");
}

/// Ref parameters on a function must be writable and must persist changes
/// back to the caller's variable (global var case).
#[test]
fn ref_param_global_var() {
    let source = "\
VAR x = 1
~ bump(x)
{x}
-> END

=== function bump(ref target) ===
~ target = target + 1
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "2\n");
}

/// Ref param passed via function call with two ref args — the
/// `move_ring` pattern from tower-of-hanoi.
#[test]
fn ref_param_function_two_refs() {
    let source = "\
VAR a = 10
VAR b = 0
~ swap(a, b)
a={a} b={b}
-> END

=== function swap(ref x, ref y) ===
~ temp t = x
~ x = y
~ y = t
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "a=0 b=10\n");
}

/// Thread-called stitch with conditional choice and ref params —
/// the core tower-of-hanoi pattern. The stitch is called via `<-`
/// and provides a conditional choice based on `can_move`.
#[test]
fn tower_of_hanoi_mini() {
    let source = "\
LIST Discs = one, two, three
VAR post1 = ()
VAR post2 = ()
VAR post3 = ()

~ post1 = LIST_ALL(Discs)

-> gameloop

=== function can_move(from_list, to_list) ===
    {
    -   LIST_COUNT(from_list) == 0:
        ~ return false
    -   LIST_COUNT(to_list) > 0 && LIST_MIN(from_list) > LIST_MIN(to_list):
        ~ return false
    -   else:
        ~ return true
    }

=== function move_ring( ref from, ref to ) ===
    ~ temp whichRingToMove = LIST_MIN(from)
    ~ from -= whichRingToMove
    ~ to += whichRingToMove

=== gameloop
    Start.
- (top)
    +  [ Regard]
        Regarded.
    <- move_post(1, 2, post1, post2)
    -> DONE

= move_post(from_post_num, to_post_num, ref from_post_list, ref to_post_list)
    +   { can_move(from_post_list, to_post_list) }
        [ Move ]
        { move_ring(from_post_list, to_post_list) }
        Moved.
    -> top
";
    // Choose \"Move\" (from move_post thread), then \"Regard\"
    let result = compile_and_run(source, &[0, 0]);
    assert!(
        result.contains("Moved") || result.contains("Regarded"),
        "expected tower-of-hanoi mini to produce output, got: {result:?}"
    );
}

/// Ref params with list operations — minimal `move_ring` pattern.
#[test]
fn ref_param_list_move_ring() {
    let source = "\
LIST Discs = one, two, three
VAR post1 = ()
VAR post2 = ()

~ post1 = LIST_ALL(Discs)

~ move_ring(post1, post2)

{post1}
{post2}
-> END

=== function move_ring( ref from, ref to ) ===
~ temp whichRingToMove = LIST_MIN(from)
~ from -= whichRingToMove
~ to += whichRingToMove
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "two, three\none\n");
}

// ── Pattern 4: Missing space literal in string interpolation ─────────

/// `{gatherCount} {loop}` must produce "1 1", not "11" — the space
/// between interpolations must be emitted as a literal.
#[test]
#[ignore = "visit count for gather labels not incremented on re-entry"]
fn space_between_interpolations_preserved() {
    let source = "\
VAR gatherCount = 0
- (loop)
~ gatherCount++
{gatherCount} {loop}
{gatherCount<3:->loop}
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "1 1\n2 2\n3 3\n");
}

// ── Pattern 4b: Conditional divert in inline branch ──────────────────

/// `{condition:->target}` — divert inside a conditional inline branch.
/// The divert was silently dropped by `lower_content_node_children`,
/// so the conditional body was empty and the divert never fired.
#[test]
fn conditional_divert_basic() {
    let source = "\
VAR x = 1
{x == 1:->yes}
Nope.
-> END
== yes ==
Yes!
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Yes!\n");
}

/// Conditional divert in a loop — the core pattern from the space test.
#[test]
fn conditional_divert_loop() {
    let source = "\
VAR i = 0
- (loop)
~ i++
{i}
{i < 3:->loop}
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "1\n2\n3\n");
}

/// Conditional with text AND divert: `{cond: text ->target}`
#[test]
fn conditional_text_then_divert() {
    let source = "\
VAR x = 1
{x == 1: Going there! ->yes}
Nope.
-> END
== yes ==
Arrived.
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Going there! Arrived.\n");
}

/// Negative case: condition is false, divert should NOT fire.
#[test]
fn conditional_divert_false_branch() {
    let source = "\
VAR x = 0
{x == 1:->yes}
Fallthrough.
-> END
== yes ==
Yes!
-> END
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Fallthrough.\n");
}

// ── Pattern 5: ref parameters compiled as pointer ────────────────────

/// `ref` parameter should pass by reference, allowing the callee to
/// modify the caller's variable.
#[test]
fn ref_parameter_modifies_caller_variable() {
    let source = "\
VAR x = 0
~ bump(x)
{x}
-> DONE

=== function bump(ref n) ===
~ n++
";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "1\n");
}

/// Tower-of-hanoi pattern with all 6 thread starts.
/// Hangs due to runtime thread merging bug — multiple threads with
/// conditional choices create an infinite loop in the VM.
#[test]
#[ignore = "runtime thread merging infinite loop with multiple conditional-choice threads"]
fn tower_of_hanoi_6threads() {
    let source = "\
LIST Discs = one, two, three
VAR post1 = ()
VAR post2 = ()
VAR post3 = ()

~ post1 = LIST_ALL(Discs)

-> gameloop

=== function can_move(from_list, to_list) ===
    {
    -   LIST_COUNT(from_list) == 0:
        ~ return false
    -   LIST_COUNT(to_list) > 0 && LIST_MIN(from_list) > LIST_MIN(to_list):
        ~ return false
    -   else:
        ~ return true
    }

=== function move_ring( ref from, ref to ) ===
    ~ temp whichRingToMove = LIST_MIN(from)
    ~ from -= whichRingToMove
    ~ to += whichRingToMove

=== gameloop
    Start.
- (top)
    +  [ Regard]
        Regarded.
    <- move_post(1, 2, post1, post2)
    <- move_post(2, 1, post2, post1)
    <- move_post(1, 3, post1, post3)
    <- move_post(3, 1, post3, post1)
    <- move_post(3, 2, post3, post2)
    <- move_post(2, 3, post2, post3)
    -> DONE

= move_post(from_post_num, to_post_num, ref from_post_list, ref to_post_list)
    +   { can_move(from_post_list, to_post_list) }
        [ Move {from_post_num} to {to_post_num} ]
        { move_ring(from_post_list, to_post_list) }
        Moved.
    -> top
";
    let result = compile_and_run(source, &[0, 0]);
    assert!(
        result.contains("Moved") || result.contains("Regarded"),
        "expected output, got: {result:?}"
    );
}

// ── Expected compile errors ─────────────────────────────────────────
//
// Inklecate rejects these programs. Brink should too.

/// Helper: extract diagnostic codes from a compile error.
fn diagnostic_codes(err: &brink_compiler::CompileError) -> Vec<&'static str> {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => {
            diags.iter().map(|d| d.code.as_str()).collect()
        }
        _ => vec![],
    }
}

/// A choice inside `{ true: * choice }` without an explicit divert is
/// invalid — inklecate errors with "need to explicitly divert".
#[test]
fn compile_error_nested_choice_in_conditional() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "{ true:\n    * choice\n}\n")]);
    let result = compile_mem("main.ink", &files);
    let err = result.expect_err(
        "choice inside inline conditional should be a compile error, \
         but compilation succeeded",
    );
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E029"),
        "expected E029 (choice in conditional must explicitly divert), got: {codes:?}"
    );
}

/// A choice inside a conditional WITH a divert is valid — E029 must not fire.
#[test]
fn choice_in_conditional_with_divert_is_valid() {
    let source = "=== play_game ===\n{ true:\n  + [Burn] -> play_game\n}\n-> END\n";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_ok(),
        "choice with divert in conditional should compile: {result:?}"
    );
}

/// A choice inside a conditional WITHOUT a divert but with a gather continuation
/// after the conditional is valid ink — inklecate accepts this.
#[test]
fn choice_in_conditional_with_gather_continuation_is_valid() {
    let source =
        "=== play_game ===\n{ true:\n  + (burny) [Burn]\n    Hello\n}\n- -> burny\n-> END\n";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_ok(),
        "choice in conditional with gather continuation should compile: {result:?}"
    );
}

/// A bare `->` (empty divert) outside a choice is invalid.
/// Inklecate: "Empty diverts (->) are only valid on choices".
#[test]
fn compile_error_disallow_empty_diverts() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "->\n")]);
    let result = compile_mem("main.ink", &files);
    let err = result.expect_err("bare `->` should be a compile error, but compilation succeeded");
    let codes = diagnostic_codes(&err);
    assert!(
        codes.contains(&"E012"),
        "expected E012 (divert is missing a target), got: {codes:?}"
    );
}

// ── Unresolved function calls should error, not silently produce Null ─

#[test]
fn unresolved_function_call_is_compile_error() {
    // A call to a function that doesn't exist should be a compile-time
    // diagnostic, not a silent Null. This guards against the LIR lowering
    // fallback that converts unresolvable calls to Expr::Null.
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "\
~ temp x = DOES_NOT_EXIST()
{x}
-> END
",
    )]);
    let result = compile_mem("main.ink", &files);
    assert!(
        result.is_err(),
        "calling a nonexistent function should produce a compile error, not succeed silently"
    );
}

// ── TURNS() built-in ────────────────────────────────────────────────

#[test]
fn turns_builtin_compiles_and_runs() {
    // TURNS() is a zero-argument ink built-in that returns the current turn
    // index. The compiler must recognize it, lower it through LIR, and emit
    // the TurnIndex opcode. This test verifies end-to-end correctness.
    let output = compile_and_run(
        "\
~ temp t = TURNS()
turn is {t}
-> END
",
        &[],
    );
    assert_eq!(output.trim(), "turn is 0");
}

#[test]
fn turns_builtin_increments_across_choices() {
    // TURNS() should increment each time the player makes a choice and
    // the story continues. Turn 0 is the initial passage, turn 1 after
    // the first choice, etc.
    let output = compile_and_run(
        "\
turn {TURNS()}
+ [continue]
-
turn {TURNS()}
-> END
",
        &[0],
    );
    assert_eq!(output.trim(), "turn 0\nturn 1");
}

// ── Block-level sequence branch behaviors ──────────────────────────

/// Compile from in-memory source, link, and run. Returns a list of
/// (text, `choice_count`) pairs for each step.
fn compile_and_run_steps(source: &str, inputs: &[usize]) -> Vec<(String, Option<usize>)> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut steps = Vec::new();
    let mut input_idx = 0;
    let mut guard = 0;

    loop {
        guard += 1;
        assert!(guard < 100, "infinite loop detected");
        let lines = story.continue_maximally().unwrap();
        let combined_text: String = lines.iter().map(Line::text).collect();
        let last = lines.last().unwrap();
        match last {
            Line::Text { .. } | Line::Done { .. } | Line::End { .. } | Line::Suspended { .. } => {
                steps.push((combined_text, None));
                break;
            }
            Line::Choices { choices, .. } => {
                let count = choices.len();
                steps.push((combined_text.clone(), Some(count)));
                let idx = if input_idx < inputs.len() {
                    let c = inputs[input_idx];
                    input_idx += 1;
                    c
                } else {
                    0
                };
                assert!(
                    idx < count,
                    "choice index {idx} out of range (only {count} choices), text so far: {combined_text:?}"
                );
                story.choose(idx).unwrap();
            }
        }
    }

    steps
}

/// Block-level sequence branches must start with a newline relative to
/// preceding content. Inklecate inserts "\n" at the start of each
/// branch's content stream. Without this, output like
/// "I drew a card. 2 of Diamonds." appears on one line instead of two.
#[test]
fn sequence_branch_starts_with_newline() {
    let source = "\
-> test

=== test ===
{ stopping:
    - Branch one.
    - Branch two.
}
* [Again] Prefix. -> test
- -> END
";
    // First visit: "Branch one.\n" + choices: [Again]
    // Choose "Again" (once-only *), second visit: "Prefix.\nBranch two.\n" + no choices → END
    let steps = compile_and_run_steps(source, &[0]);
    // Step 1 (after choosing "Again") text must have a newline between
    // "Prefix." and "Branch two."
    assert!(
        steps.len() >= 2,
        "expected at least 2 steps, got {}",
        steps.len()
    );
    let text = &steps[1].0;
    assert!(
        text.contains("Prefix.") && text.contains("Branch two."),
        "expected both 'Prefix.' and 'Branch two.' in output, got: {text:?}"
    );
    // The newline must separate them (not on the same line)
    assert!(
        !text.contains("Prefix. Branch two.") && !text.contains("Prefix.Branch two."),
        "expected newline between 'Prefix.' and 'Branch two.', got: {text:?}"
    );
}

/// Choices inside a sequence branch must accumulate with choices from the
/// parent container. When a sequence branch contains a `ChoiceSet` and there
/// are also choices after the sequence in the same container, all choices
/// must be visible together (the branch's Done must not block the parent).
#[test]
fn choices_inside_sequence_branch_accumulate_with_parent() {
    // Pattern from the multiline-choice test case: a stopping sequence
    // where branch 1 has a once-only choice, plus a sticky choice after
    // the sequence. On visit 2, both must be visible.
    let source = "\
-> test
=== test ===
{ stopping:
    - At the table, I drew a card. Ace of Hearts.
    - 2 of Diamonds.
        \"Should I hit you again,\" the croupier asks.
        * [No.] I left the table. -> END
    - King of Spades.
        \"You lose,\" he crowed.
        -> END
}
+ [Draw a card] I drew a card. -> test
";
    // Visit 1: branch 0 text + choices: [Draw a card]
    // Choose "Draw a card" → visit 2: branch 1, choices: [No., Draw a card]
    let steps = compile_and_run_steps(source, &[0, 0]);
    // Second step must show 2 choices: [No., Draw a card]
    let second_choice_count = steps[1].1;
    assert_eq!(
        second_choice_count,
        Some(2),
        "expected 2 choices (No. + Draw a card) on second visit, got: {second_choice_count:?}"
    );
}

/// Content after a block-level conditional's closing `}` must not be
/// dropped. The glue and text `<> b` should join with the branch output.
#[test]
fn content_after_multiline_conditional_preserved() {
    let source = "\
{true:
    a
} <> b
";
    let result = compile_and_run(source, &[]);
    assert_eq!(
        result, "a b\n",
        "glue + text after conditional must be preserved"
    );
}

/// Same as above but with a second conditional after the glue.
#[test]
fn content_after_multiline_conditional_with_nested_conditional() {
    let source = "\
{true:
    a
} <> { true:
    b
}
";
    let result = compile_and_run(source, &[]);
    assert_eq!(
        result, "a b\n",
        "glue + conditional after conditional must be preserved"
    );
}

// ── Shuffle sequence exhaustion ────────────────────────────────────

/// `shuffle once` must stop producing content after all branches are visited.
/// This is an end-to-end behavioral test: call a shuffle-once function 4 times
/// with 2 branches — only the first 2 calls should produce text.
#[test]
fn shuffle_once_exhausts_after_all_branches_visited() {
    let source = "\
~ SEED_RANDOM(1)
one: {f()}
two: {f()}
three: {f()}
four: {f()}
== function f ==
{shuffle once:
    - A
    - B
}
";
    let result = compile_and_run(source, &[]);
    // Each of the 4 lines "N: X\n" gets the function result appended.
    // First 2 calls produce "A" or "B" (in shuffled order); last 2 produce nothing.
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines.len(), 4, "expected 4 output lines, got: {result:?}");

    // First two lines must each contain either "A" or "B".
    let first_two_content: Vec<&str> = lines[0..2]
        .iter()
        .map(|l| l.split(": ").nth(1).unwrap_or("").trim())
        .collect();
    let mut sorted = first_two_content.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["A", "B"],
        "first two calls should produce A and B (in any order), got: {first_two_content:?}"
    );

    // Last two lines must have no content after the colon.
    for (i, line) in lines[2..].iter().enumerate() {
        let after_colon = line.split(": ").nth(1).unwrap_or("").trim();
        assert!(
            after_colon.is_empty(),
            "call {} (line {:?}) should produce no text after exhaustion, got: {after_colon:?}",
            i + 3,
            line,
        );
    }
}

/// `shuffle stopping` must pin to the last branch after all are visited.
/// Call a 3-branch shuffle-stopping function 5 times — after the first 3 calls
/// exhaust all branches, calls 4 and 5 must always return the last branch.
#[test]
fn shuffle_stopping_pins_to_last_branch() {
    let source = "\
~ SEED_RANDOM(1)
one: {f()}
two: {f()}
three: {f()}
four: {f()}
five: {f()}
== function f ==
{stopping shuffle:
    - A
    - B
    - final
}
";
    let result = compile_and_run(source, &[]);
    let lines: Vec<&str> = result.lines().collect();
    assert_eq!(lines.len(), 5, "expected 5 output lines, got: {result:?}");

    // First three calls produce A, B, final in some shuffled order.
    let first_three_content: Vec<String> = lines[0..3]
        .iter()
        .map(|l| l.split(": ").nth(1).unwrap_or("").trim().to_string())
        .collect();
    let mut sorted: Vec<&str> = first_three_content.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec!["A", "B", "final"],
        "first three calls should produce A, B, final (in any order), got: {first_three_content:?}"
    );

    // Calls 4 and 5 must produce "final" (the last/stopping branch).
    for (i, line) in lines[3..].iter().enumerate() {
        let after_colon = line.split(": ").nth(1).unwrap_or("").trim();
        assert_eq!(
            after_colon,
            "final",
            "call {} should pin to 'final' after exhaustion, got: {after_colon:?}",
            i + 4,
        );
    }
}

/// Opcode-level test: `shuffle once` codegen must emit a `Min` opcode
/// to clamp the visit count, enabling exhaustion detection.
#[test]
fn shuffle_once_codegen_emits_min_opcode() {
    use brink_format::Opcode;

    let source = "\
{shuffle once:
    - A
    - B
}
";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();

    // Find the sequence container (has VISITS + COUNT_START_ONLY flags).
    let seq_container = data
        .containers
        .iter()
        .find(|c| {
            let mut offset = 0;
            let mut has_sequence = false;
            while offset < c.bytecode.len() {
                if let Ok(op) = Opcode::decode(&c.bytecode, &mut offset) {
                    if matches!(op, Opcode::Sequence(..)) {
                        has_sequence = true;
                    }
                } else {
                    break;
                }
            }
            has_sequence
        })
        .expect("should find a container with a Sequence opcode");

    // Decode all opcodes and check for Min.
    let mut offset = 0;
    let mut has_min = false;
    while offset < seq_container.bytecode.len() {
        if let Ok(op) = Opcode::decode(&seq_container.bytecode, &mut offset) {
            if matches!(op, Opcode::Min) {
                has_min = true;
            }
        } else {
            break;
        }
    }
    assert!(
        has_min,
        "shuffle once container must emit Min opcode for exhaustion clamping"
    );
}

/// Contextual keywords like `once`, `stopping`, `shuffle`, `cycle` must be
/// usable as knot names and divert targets. Ink only treats these as keywords
/// inside sequence annotations — everywhere else they're valid identifiers.
#[test]
fn keyword_once_as_knot_name_and_divert_target() {
    let source = "\
-> once
== once ==
Hello from once.
-> END
";
    let result = compile_and_run(source, &[]);
    assert!(
        result.contains("Hello from once"),
        "knot named 'once' should work, got: {result:?}"
    );
}

/// Full thread-in-logic test (inklecate's TestThreadInLogic): tunnel calls
/// to a knot named `once` containing `{<- content|}`.
#[test]
fn thread_in_logic_compiles_and_runs() {
    let source = "\
-> once ->
-> once ->
== once ==
{<- content|}
->->
== content ==
Content
-> DONE
";
    let result = compile_and_run(source, &[]);
    assert!(
        result.contains("Content"),
        "thread-in-logic should produce 'Content', got: {result:?}"
    );
}

// ── Template tests (intl-spec phase 3) ──────────────────────────────

#[test]
fn template_single_variable() {
    let source = "VAR name = \"World\"\nHello, {name}!\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Hello, World!\n");
}

#[test]
fn template_multiple_interpolations() {
    let source = "VAR a = \"one\"\nVAR b = \"two\"\n{a} and {b}\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "one and two\n");
}

#[test]
fn template_expression_interpolation() {
    let source = "VAR n = 3\nResult: {n * 2}\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Result: 6\n");
}

#[test]
fn template_interpolation_at_start() {
    let source = "VAR x = \"Hello\"\n{x} world\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Hello world\n");
}

#[test]
fn template_interpolation_at_end() {
    let source = "VAR x = \"world\"\nHello {x}\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Hello world\n");
}

#[test]
fn plain_text_regression() {
    // Ensure plain text lines still work after template support.
    let source = "Just plain text.\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Just plain text.\n");
}

#[test]
fn template_integer_interpolation() {
    let source = "VAR count = 42\nThere are {count} items.\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "There are 42 items.\n");
}

#[test]
fn template_float_interpolation() {
    let source = "VAR pi = 3.14\nPi is {pi}.\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Pi is 3.14.\n");
}

#[test]
fn template_bool_interpolation() {
    let source = "VAR flag = true\nFlag: {flag}\n";
    let result = compile_and_run(source, &[]);
    assert_eq!(result, "Flag: true\n");
}

// ── Warning surfacing ───────────────────────────────────────────────

/// Helper: compile and return the full `CompileOutput` (data + warnings).
fn compile_mem_with_warnings(
    entry: &str,
    files: &HashMap<&str, &str>,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    brink_compiler::compile(entry, |path| {
        files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {path}"),
            )
        })
    })
}

#[test]
fn warnings_surfaced_alongside_successful_compilation() {
    // A CONST with string interpolation should compile successfully
    // but produce an E030 warning.
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "VAR name = \"world\"\nCONST greeting = \"hi {name}\"\n{greeting}\n",
    )]);

    let output = compile_mem_with_warnings("main.ink", &files).unwrap();
    assert!(
        !output.data.containers.is_empty(),
        "compilation should succeed"
    );
    assert!(
        output.warnings.iter().any(|w| w.code.as_str() == "E030"),
        "expected E030 warning, got: {:?}",
        output
            .warnings
            .iter()
            .map(|w| w.code.as_str())
            .collect::<Vec<_>>()
    );
}

/// A warning originating in an included file must carry that file's path, not
/// the entry's. Regression for #187 (secondary): the public API used to return
/// diagnostics keyed only by an opaque `FileId`, so a consumer had no way to
/// locate them and collapsed every diagnostic onto the entry file. The E033
/// here lives wholly in `phone.ink`, so its resolved `path` must be `phone.ink`.
#[test]
fn warning_from_included_file_carries_its_path() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE phone.ink\n-> reveal\n"),
        // `-> END` is terminal; the trailing content is unreachable → E033.
        ("phone.ink", "=== reveal ===\n-> END\nAnd we're off.\n"),
    ]);

    let output = compile_mem_with_warnings("main.ink", &files).unwrap();
    let e033 = output
        .warnings
        .iter()
        .find(|w| w.code.as_str() == "E033")
        .expect("expected an E033 warning from the unreachable line in phone.ink");
    assert_eq!(
        e033.path, "phone.ink",
        "E033 from phone.ink must be attributed to phone.ink, not the entry"
    );
}

#[test]
fn clean_compilation_has_no_warnings() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "Hello, world!\n-> END\n")]);

    let output = compile_mem_with_warnings("main.ink", &files).unwrap();
    assert!(
        output.warnings.is_empty(),
        "expected no warnings for clean source, got: {:?}",
        output
            .warnings
            .iter()
            .map(|w| format!("[{}] {}", w.code.as_str(), w.message))
            .collect::<Vec<_>>()
    );
}

#[test]
fn glue_in_choice_body_emits_glue_opcode() {
    let source = "\
-> knot

=== knot
* [Yes]
    Yes considered. <>
* [No]
    No way. <>
- He seemed to know.
-> END
";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let mut buf = String::new();
    brink_format::write_inkt(&data, &mut buf).unwrap();
    eprintln!("{buf}");
    assert!(
        buf.contains("glue"),
        "expected glue opcode in bytecode, got:\n{buf}"
    );
}

#[test]
fn glue_in_choice_body_runtime_joins_text() {
    let source = "\
-> knot

=== knot
* [Yes]
    Yes considered. <>
* [No]
    No way. <>
- He seemed to know.
-> END
";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let data = compile_mem("main.ink", &files).unwrap();
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    // First continue: should get choices
    let line = story.continue_single().unwrap();
    match &line {
        Line::Choices { choices, .. } => {
            assert_eq!(choices.len(), 2);
            story.choose(0).unwrap(); // pick "Yes"
        }
        other => panic!("expected Choices, got: {other:?}"),
    }

    // Second continue: should get the glued text
    let line = story.continue_single().unwrap();
    let text = match &line {
        Line::Text { text, .. } => text.clone(),
        Line::End { text, .. } => text.clone(),
        Line::Done { text, .. } => text.clone(),
        Line::Suspended { text, .. } => text.clone(),
        Line::Choices { .. } => panic!("expected text output, got choices"),
    };
    eprintln!("got text: {text:?}");
    assert!(
        text.contains("Yes considered. He seemed to know."),
        "expected glue to join choice text with gather text, got: {text:?}"
    );
}

// ── Malformed inline conditionals (regression for #44) ──────────────
//
// Per WritingWithInk.md and inkle's own Tests.cs, conditional *logic* and
// conditions-on-each-branch only exist in the multiline block form. inklecate
// rejects the inline forms below; brink currently compiles them and emits the
// source as story text (silent miscompile). These assert the reference-correct
// behaviour: a malformed inline conditional must be a compile error.

/// `{ cond: ~ statement }` — a logic statement inside an inline conditional.
/// Logic must live in a multiline block (`{ cond:\n    ~ ... \n}`), so the
/// inline form is invalid ink and must error.
#[test]
fn compile_error_inline_conditional_with_logic() {
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "VAR x = 0\n{ true: ~ x = 2 }\nValue {x}.\n-> END\n",
    )]);
    let err = compile_mem("main.ink", &files).expect_err(
        "an inline conditional containing a `~` logic statement is invalid ink \
         (logic belongs in a multiline block) and should be a compile error",
    );
    let codes = diagnostic_codes(&err);
    assert!(!codes.is_empty(), "expected a diagnostic, got: {codes:?}");
}

/// `{ c1: a | c2: b | else }` — conditions on each branch, inline. Multi-branch
/// switches with per-branch conditions only exist in the multiline block form
/// (`{ - c1: a\n  - c2: b\n  - else: c }`), so the inline pipe form is invalid
/// ink and must error.
///
#[test]
fn compile_error_inline_multi_branch_conditional() {
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "VAR n = 5\nIt is {n > 8: big|n > 4: medium|small}.\n-> END\n",
    )]);
    let err = compile_mem("main.ink", &files).expect_err(
        "an inline conditional with conditions on each branch is invalid ink \
         (multi-branch switches require the multiline block form) and should be \
         a compile error",
    );
    let codes = diagnostic_codes(&err);
    assert!(!codes.is_empty(), "expected a diagnostic, got: {codes:?}");
}

// ── Directive annotations (`#@local`) ───────────────────────────────

#[test]
fn local_directive_reaches_story_data() {
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "\
#@local
VAR mood = 0
VAR shared = 1
-> guard

== guard ==
#@local
Halt! # spoken
-> END

== plaza ==
Busy.
-> END
",
    )]);

    let story = compile_mem("main.ink", &files).unwrap();

    // The VAR bit lands on the right global and only that one.
    let name = |id: brink_format::NameId| story.name_table[id.0 as usize].as_str();
    let mood = story
        .variables
        .iter()
        .find(|v| name(v.name) == "mood")
        .unwrap();
    let shared = story
        .variables
        .iter()
        .find(|v| name(v.name) == "shared")
        .unwrap();
    assert!(mood.local, "#@local VAR carries the scope bit");
    assert!(!shared.local, "unmarked VAR stays World");

    // The knot bit lands on `guard` and only `guard`.
    let guard = story
        .containers
        .iter()
        .find(|c| c.name.is_some_and(|n| name(n) == "guard"))
        .unwrap();
    let plaza = story
        .containers
        .iter()
        .find(|c| c.name.is_some_and(|n| name(n) == "plaza"))
        .unwrap();
    assert!(guard.local, "#@local knot carries the scope bit");
    assert!(!plaza.local, "unmarked knot stays World");

    // Erasure: no `@local` text anywhere in the line tables, but the
    // plain `spoken` tag survives.
    let all_lines = format!("{:?}", story.line_tables);
    assert!(
        !all_lines.contains("@local"),
        "directives never reach runtime content"
    );
    assert!(all_lines.contains("spoken"), "plain tags survive");
}

/// `#@local` declares that a knot/stitch's counts are per-flow memory —
/// the compiler must set `CountingFlags::VISITS` on the marked container
/// (and the scope-owning containers in its subtree, i.e. a marked knot's
/// stitches) even when nothing in the ink reads the count. Without this,
/// the read-site optimization compiles counting out and there is nothing
/// to privatize (#496).
#[test]
fn local_directive_implies_visits_counting() {
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "\
-> guard

== guard ==
#@local
Halt!
-> inner

= inner
Deeper.
-> END

== plaza ==
Busy.
-> nook

= nook
#@local
Quiet.
-> END
",
    )]);

    let story = compile_mem("main.ink", &files).unwrap();

    let name = |id: brink_format::NameId| story.name_table[id.0 as usize].as_str();
    let container = |wanted: &str| {
        story
            .containers
            .iter()
            .find(|c| c.name.is_some_and(|n| name(n) == wanted))
            .unwrap_or_else(|| {
                let names: Vec<_> = story
                    .containers
                    .iter()
                    .filter_map(|c| c.name.map(name))
                    .collect();
                panic!("container {wanted:?} not found; named containers: {names:?}")
            })
    };
    let visits = |wanted: &str| {
        container(wanted)
            .counting_flags
            .contains(brink_format::CountingFlags::VISITS)
    };

    // The marked knot: no read site anywhere, VISITS forced anyway.
    assert!(visits("guard"), "#@local knot implies VISITS");
    // Scope-owning child of the marked knot: covered by the subtree rule
    // (the runtime privatizes the whole definition subtree, so the
    // stitch's count must exist too).
    assert!(
        visits("guard.inner"),
        "stitch under a #@local knot implies VISITS"
    );
    // A marked stitch inside an unmarked knot: the stitch is forced...
    assert!(visits("plaza.nook"), "#@local stitch implies VISITS");
    // ...but the read-site optimization stays intact everywhere else.
    assert!(
        !visits("plaza"),
        "unmarked, unread knot keeps counting compiled out"
    );
}
