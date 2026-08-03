//! End-to-end FS-2 `await` compiler slice tests
//! (docs/flow-suspension-spec.md §3/§5, issue #928).
//!
//! Exercises the full pipeline through `brink_compiler::compile_with_options`,
//! proving the concrete consumer path a CLI/library caller reaches:
//! - `await` grammar → HIR (parse succeeds);
//! - strict-ink gate (`E051`) — `await` is a brink extension;
//! - the LIR lowering fence (`E052`) — every `await` construct is fenced until
//!   the FS-3 runtime lands;
//! - the effect-free purity gate (`E105`) — a condition that transitively
//!   writes a global (or performs an effectful call) is rejected, while a
//!   read-only condition passes the gate (it still hits the fence).

#![allow(clippy::panic)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_ir::DiagnosticCode;

fn compile_mem_with_dialect(
    source: &str,
    dialect: Dialect,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect,
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

fn has_code(diags: &[brink_compiler::ResolvedDiagnostic], code: DiagnosticCode) -> bool {
    diags.iter().any(|d| d.code == code)
}

// ── strict-ink rejects `await` (E051) ────────────────────────────────

#[test]
fn strict_ink_rejects_await_logic_line() {
    let source = "VAR gold = 0\n=== start ===\n~ await gold > 100\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E051
            && d.message.contains("brink extension")
            && d.message.contains("await")),
        "{diags:?}"
    );
}

#[test]
fn strict_ink_rejects_while_await() {
    let source =
        "VAR alarm = false\n=== start ===\n~ {\nwhile await alarm {\nalarm = false\n}\n}\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(has_code(&diags, DiagnosticCode::E051), "{diags:?}");
}

// ── brink fences every `await` at lowering (E052) ────────────────────

#[test]
fn brink_fences_await_logic_line() {
    let source = "VAR gold = 0\n=== start ===\n~ await gold > 100\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E052 && d.message.contains("await")),
        "{diags:?}"
    );
}

#[test]
fn brink_fences_await_inside_block() {
    let source = "VAR gold = 0\n=== start ===\n~ {\nawait gold > 100\n}\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(has_code(&diags, DiagnosticCode::E052), "{diags:?}");
}

#[test]
fn brink_fences_while_await() {
    let source =
        "VAR alarm = false\n=== start ===\n~ {\nwhile await alarm {\nalarm = false\n}\n}\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(has_code(&diags, DiagnosticCode::E052), "{diags:?}");
}

// ── the purity gate (E105) ───────────────────────────────────────────

/// A read-only condition (`gold > 100`) passes the purity gate — it hits the
/// lowering fence (E052) but never the purity error (E105).
#[test]
fn brink_pure_condition_passes_purity_gate() {
    let source = "VAR gold = 0\n=== start ===\n~ await gold > 100\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        has_code(&diags, DiagnosticCode::E052),
        "expected fence: {diags:?}"
    );
    assert!(
        !has_code(&diags, DiagnosticCode::E105),
        "a read-only condition must not trip the purity gate: {diags:?}"
    );
}

/// A bare fn-value reference used as a dynamic condition
/// (`await ready`, no call syntax) is read-only by construction (spec §3) —
/// no E105.
#[test]
fn brink_bare_reference_condition_passes_purity_gate() {
    let source = "VAR ready = false\n=== start ===\n~ await ready\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        !has_code(&diags, DiagnosticCode::E105),
        "a bare reference condition must not trip the purity gate: {diags:?}"
    );
}

/// A condition that transitively **writes** a global (calling a function that
/// assigns a VAR) is not effect-free → E105.
#[test]
fn brink_effectful_condition_writing_global_is_rejected() {
    let source = concat!(
        "VAR alarm = false\n",
        "=== function raise_alarm() ===\n",
        "~ alarm = true\n",
        "~ return true\n",
        "=== start ===\n",
        "~ await raise_alarm()\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "an effectful condition must trip the purity gate: {diags:?}"
    );
}

/// Transitive write, two hops out: the condition calls `outer()`, which calls
/// `inner()`, which writes a global. The effect row is transitively closed, so
/// `outer`'s row carries the write even though `outer` never touches the global
/// directly — E105 must still fire (PR #935 review: transitive-write coverage).
#[test]
fn brink_effectful_condition_writing_global_two_hops_is_rejected() {
    let source = concat!(
        "VAR sirens = false\n",
        "=== function inner() ===\n",
        "~ sirens = true\n",
        "~ return true\n",
        "=== function outer() ===\n",
        "~ return inner()\n",
        "=== start ===\n",
        "~ await outer()\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a two-hop transitive write must trip the purity gate: {diags:?}"
    );
}

/// An effectful call nested inside a **struct-construction** condition
/// (`await Flag#{on: raise_alarm()}`) must trip E105 — the field initializer is
/// evaluated, so its write is observable on re-evaluation. Regression for the
/// PR #935 review item: `Expr::StructLiteral` was a non-recursing leaf in the
/// purity walk, so this write slipped past the gate.
#[test]
fn brink_effectful_call_in_struct_literal_condition_is_rejected() {
    let source = concat!(
        "STRUCT Flag = #{on: bool}\n",
        "VAR sirens = false\n",
        "=== function raise_alarm() ===\n",
        "~ sirens = true\n",
        "~ return true\n",
        "=== start ===\n",
        "~ await Flag#{on: raise_alarm()}\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "an effectful call nested in a struct-construction condition must trip \
         the purity gate: {diags:?}"
    );
}

/// A condition calling a **pure** function (one that only reads a global)
/// stays read-only → no E105 (still fenced by E052).
#[test]
fn brink_condition_calling_pure_function_passes_purity_gate() {
    let source = concat!(
        "VAR alarm = false\n",
        "=== function alarm_raised() ===\n",
        "~ return alarm\n",
        "=== start ===\n",
        "~ await alarm_raised()\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        !has_code(&diags, DiagnosticCode::E105),
        "a pure-function condition must not trip the purity gate: {diags:?}"
    );
}

/// NS-A6 (issue #1112, docs/stdlib-spec.md §7 — the ruled free
/// consequence): a wake condition calling a draw-bearing function is
/// excluded by the existing purity machinery, because the draw is an
/// ordinary write (to the RNG cell) in the callee's row. A re-evaluated
/// draw would be re-roll-unstable — E105 is the correct rejection.
#[test]
fn brink_draw_bearing_condition_is_rejected_by_the_purity_gate() {
    let source = concat!(
        "=== function lucky() ===\n",
        "~ return chance(0.5)\n",
        "=== start ===\n",
        "~ await lucky()\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a draw-bearing condition must trip the purity gate: {diags:?}"
    );
}

/// Issue #1128 (the wake-gate gap NS-A6's build disclosed): a draw via an
/// *unresolved intrinsic directly in* the condition expression —
/// `await chance(0.5)`, no intermediate def — must be E105-flagged too.
/// Before the fix, `await_purity`'s walk only consulted resolved callees'
/// effect rows, so the direct-intrinsic shape slipped through even though
/// the equivalent call through a def (the test above) was rejected. Now the
/// walk consults the same shared intrinsic effect table `infer_intrinsic`
/// harvests from (`brink-analyzer`'s `infer::intrinsics` — one table, no
/// second list to drift): a draw is an RNG-cell write.
#[test]
fn regression_1128_await_chance_draw() {
    let source = "=== start ===\n~ await chance(0.5)\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a direct draw-bearing intrinsic in the condition must trip the \
         purity gate: {diags:?}"
    );
}

/// Issue #1128, the sibling shape (present since NS-A1): a fault-bearing —
/// and here also receiver-mutating — intrinsic directly in the condition
/// (`await pop(a)`). Same gap, same fix: the shared intrinsic effect table
/// marks `pop` fault-bearing (and its receiver write), so re-evaluating the
/// condition is observable and E105 fires.
#[test]
fn regression_1128_await_pop_fault() {
    let source = "VAR a = #[1, 2]\n=== start ===\n~ await pop(a)\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a direct fault-bearing intrinsic in the condition must trip the \
         purity gate: {diags:?}"
    );
}

/// Review finding on #1840 Q4's registration slice: `register` is a
/// direct-unresolved-intrinsic write to the conventions-registry cell
/// (`infer::intrinsics::intrinsic_effects`'s `conventions_write` bit),
/// exactly the same shape as the RNG-cell write `chance` performs above —
/// so `await register(...)` must trip E105 too. `call_is_effectful`'s
/// direct-intrinsic arm originally consulted only `rng_write` and `faults`,
/// missing the newer `conventions_write` atom entirely; a `register` call
/// directly in a wake condition silently passed the purity gate.
#[test]
fn regression_1840_await_register_write() {
    let source = concat!(
        "=== function scene() ===\n",
        "~ return 1\n",
        "=== start ===\n",
        "~ await register(scene)\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a direct conventions-registry-write intrinsic in the condition \
         must trip the purity gate: {diags:?}"
    );
}

/// The other side of the #1128 coin: a **total, read-only** unresolved
/// intrinsic in the condition (`string(…)` — no fault path, no draw, no
/// write) stays outside every set in the shared table and must NOT trip the
/// gate — the table consult is a targeted fix, not a blanket
/// unresolved-call rejection.
#[test]
fn regression_1128_total_intrinsic_condition_still_passes() {
    let source = "VAR mood = 1\n=== start ===\n~ await string(mood) == \"2\"\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        !has_code(&diags, DiagnosticCode::E105),
        "a total read-only intrinsic condition must not trip the purity \
         gate: {diags:?}"
    );
}

// ── higher-order call sites (issue #1748, post-#1680 hole instantiation) ──

/// Issue #1748: A function that calls through a fn-typed parameter has a
/// pessimal row (one with a hole, post-#1680). When that function is called
/// in an await condition, the purity gate checks whether the call is
/// effectful. This test captures the higher-order call-site scenario where
/// the callee (`apply_fn`) itself is pessimal due to the fn parameter's row
/// variable, proving that the gate correctly rejects pessimal rows even after
/// the `is_pessimal()` change from opaque-only checking.
///
/// This was entirely untested before — the suite had direct calls to effects
/// but no higher-order scenarios mixing functions and await.
#[test]
fn brink_await_purity_gate_rejects_higher_order_call_with_fn_param() {
    let source = concat!(
        "=== function pure_fn() ===\n",
        "~ return true\n",
        "=== function apply_fn(callback: fn(): bool) ===\n",
        "~ return callback()\n",
        "=== start ===\n",
        "~ await apply_fn(pure_fn)\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    // A function calling through an fn-typed parameter is pessimal (has a hole
    // in its row, per `EffectRow::holes` — crates/internal/brink-analyzer/src/
    // infer/effects.rs:141: "a higher-order definition read on its own is
    // still pessimal"). The purity gate correctly rejects it with E105, even
    // though `pure_fn` itself is pure — the gate sees only `apply_fn`'s
    // pessimal row, not the instantiated row at the specific call site. This
    // is by design (docs/effects-spec.md §6.1b, "Row variables on fn-typed
    // params"): precision only arrives one hop up, in `solve_scc_effects`,
    // which is not consulted at an `await` call site.
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a higher-order call site is pessimal and must trip the purity gate: {diags:?}"
    );
}

/// Issue #1748, the impure higher-order variant: passing an *effectful*
/// function (`raise_alarm`, which writes a VAR) through the same fn-typed
/// parameter is also rejected. Paired with the pure variant above so both
/// higher-order purity outcomes are covered, matching the pure/impure
/// pairing convention every other purity scenario in this file follows.
#[test]
fn brink_await_purity_gate_rejects_higher_order_call_with_impure_fn_param() {
    let source = concat!(
        "VAR alarm = false\n",
        "=== function raise_alarm() ===\n",
        "~ alarm = true\n",
        "~ return true\n",
        "=== function apply_fn(callback: fn(): bool) ===\n",
        "~ return callback()\n",
        "=== start ===\n",
        "~ await apply_fn(raise_alarm)\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E105),
        "a higher-order call site with an effectful argument must trip the \
         purity gate: {diags:?}"
    );
}

/// Issue #1748: a real control for the two tests above. `holds_fn` takes a
/// fn-typed parameter (so it is the same *shape* as `apply_fn`) but never
/// calls through it — a hole is only inserted on call-through
/// (`crates/internal/brink-analyzer/src/infer/body.rs:882`,
/// `resolve_pending_value_calls`'s `ValueCallOrigin::Param` arm), so
/// `holds_fn`'s row stays ground and total. This proves the E105 rejection
/// above is specific to call-through pessimality, not merely to a def
/// having a fn-typed parameter in its signature.
#[test]
fn brink_await_purity_gate_passes_fn_param_never_called_through() {
    let source = concat!(
        "=== function holds_fn(callback: fn(): bool) ===\n",
        "~ return true\n",
        "=== function pure_fn() ===\n",
        "~ return true\n",
        "=== start ===\n",
        "~ await holds_fn(pure_fn)\n",
        "-> END\n",
    );
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        !has_code(&diags, DiagnosticCode::E105),
        "a fn-typed parameter that is never called through must not trip the \
         purity gate: {diags:?}"
    );
}
