//! Strict typed-mode (`types = strict`) sweep of the tier-1 **native**
//! golden corpus — the coverage hole issue #1882 was filed to close.
//!
//! # Why this file exists
//!
//! `tier1_native.rs` compiles every `tests/tier1-native/<case>/story.brink`
//! through `brink_compiler::compile_path`, which uses
//! `AnalysisOptions::default()` — `dialect = StrictInk`, `types = None`,
//! so `AnalysisOptions::type_policy()` resolves to `TypePolicy::Gradual`
//! (issue #1127's dialect-keyed default). `brink_analyzer::
//! strict_diagnostics` returns immediately under `Gradual`, so **no part of
//! the TM-3 strict pass — `strict::check`'s `E063`/`E065`/`E066` family —
//! has ever been evaluated against the native corpus**, even though a real
//! `.brink` project that sets `dialect = brink` in its `brink.toml` gets
//! `Strict` by default.
//!
//! That hole is what let a run of native strict-typing bugs surface one at
//! a time instead of as a batch (#1849, #1864, #1877, #1881, #1895, #1900,
//! #1902 — every one of them a question the strict checker would have been
//! asked had it run over this corpus). This file asks all of those
//! questions at once, on every commit.
//!
//! # Why a recorded baseline rather than "must be clean"
//!
//! Turning strict on over the corpus for the first time produces findings —
//! that is the point, not a setback. Per CLAUDE.md, a check that trips on
//! real corpus code means the **check** is wrong (or reality differs from
//! the contract), so the corpus is *not* edited to make this green and the
//! diagnostics are *not* silenced. Instead this test pins the exact set of
//! strict findings the corpus currently produces, so that:
//!
//! - a **new** strict finding (a regression, or a newly-written fixture
//!   that trips the checker) fails this test and gets triaged;
//! - a **fixed** finding also fails this test, forcing the baseline to
//!   shrink deliberately rather than rotting.
//!
//! Every baseline row is classified below as a **true positive** (the
//! diagnostic is right and the fixture/compiler genuinely has the problem)
//! or a **checker gap** (the diagnostic is wrong and the analyzer needs
//! fixing), each with the issue tracking it. Nothing here is "accepted";
//! the baseline is a worklist with a test attached.
//!
//! The baseline keys on `(case, code, message)` and deliberately drops the
//! diagnostic's byte range — a range shifts whenever a fixture gains a
//! line, which would make this test fail for reasons that have nothing to
//! do with typing. The message already names the offending symbol, which is
//! the part a triager needs.
//!
//! # Relationship to the oracle
//!
//! None. This corpus is self-referential and has no C# counterpart (see
//! `tier1_native.rs`'s own module doc); `RATCHET_EPISODE_COUNT` in
//! `oracle_snapshots.rs` is a separate number and must never be conflated
//! with the counts here.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use brink_compiler::{AnalysisOptions, CompileError, DiagnosticCode, Dialect, TypePolicy};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-native")
}

/// The strict findings `tests/tier1-native/` currently produces, as
/// `(case, code, message)` — sorted, exactly as [`strict_findings`] returns
/// them. Grouped by root cause, each group carrying its classification and —
/// where the classification is "defect" rather than "expected" — the issue
/// that tracks it (#1909, #1910, #1912, all filed from #1882's first
/// sweep; #1911 is FIXED — see Group D — and no `BASELINE` row references
/// it any more).
///
/// **Group A — `content`-typed parameters (`annotations-element`), tracked
/// by issue #1912.** This is the fixture issue #1864 named when it filed the
/// flattening problem, and the shape #1882 predicted would fire the moment
/// strict ran here.
///
/// - The two `interior` rows are a **true positive against the compiler**:
///   `hir::lower_native::element::try_claim` binds a claimed heading's
///   captured run as a plain `Expr::String` regardless of the receiving
///   parameter's declared type (`tier1_native.rs`'s `annotations_element`
///   doc says so in as many words), so the compiler synthesizes a call
///   whose argument type contradicts the handler signature it is calling.
///   Closing it is issue #1839's captured-run-to-`FragmentRef` scope.
/// - The `radio` row is the same mismatch written by hand — a `string`
///   literal passed to a `content` parameter. Whether `string` should widen
///   to `content` at all is the open half of that question.
/// - `radio`'s **return type** escaping as `Unknown` is a **checker gap**:
///   `radio`'s body is `return text;` where `text: content`, so the return
///   type is exactly the annotated parameter type. Reading a `content`-typed
///   binding as a value yields `Unknown` rather than `content` (reduced:
///   `fn passthru(t: content) { return t; }` reports the same, while
///   `fn passthru(t: content): content { return t; }` is clean).
///
/// **Group B — UFCS method-call results (`ufcs`), tracked by issue #1909.**
/// A **checker gap**: the desugared method-call spelling loses its result
/// type where the direct spelling keeps it. Reduced to a single file,
/// `fn f() { let n = 21; return double(n); }` is clean while
/// `fn f() { let n = 21; return n.double(); }` reports `E065` — identical
/// bodies, only the call spelling differs. `describe_double`
/// (`FreeFnDesugar`) and `tally` (`m.len()`, `PreludeDesugar`) are the two
/// forms of it here. Distinct from issue #1483, which is the
/// receiver-type-unknown direction; here the receiver's type is known and
/// resolution succeeds.
///
/// `bump`/`heal`'s parameters are a different, **expected** row: their
/// bodies genuinely place no constraint on the parameters (`n = n +
/// amount`), and §2 of `docs/typed-mode-spec.md` forbids call-site-driven
/// inference, so `Unknown` is the specified outcome — the fixture is
/// written in gradual style and would need annotations to be strict-clean.
///
/// **Group C — pure verb-layer results (`lambda-verbs`,
/// `fn-value-bare-name`), tracked by issue #1910.**
/// A **checker gap**: `map`/`filter`/`fold`/`filter_map`/`map_each` results
/// escape as `Unknown` when no surrounding annotation ascribes them, even
/// where the element type is unambiguous (`fold(items, 0, |a, b| a + b)`
/// over `Array<int>` with an `int` seed). Ascribing the enclosing return
/// (`fn f(): Array<int>`) makes the same body clean, which is what isolates
/// this to the intrinsic typing rules for the verb layer rather than to the
/// lambdas. A lambda literal bound to a local (`let f = |x| x + 1;`) has the
/// same shape: the binding escapes as `Unknown` instead of taking the
/// lambda's own `fn(T…): R` type.
///
/// **Group D — string concatenation (`for-k-v`), issue #1911, FIXED.**
/// `keys + ":" + total` used to mark the `int` local `total` as
/// `Conflicted` (`E066`), because `+` was unified as a same-type operator
/// with no exception for display concatenation. `"t:" + total` on a plain
/// `int` reproduced it with no map or loop involved. This was legal,
/// running ink — the fixture's golden transcript proves it — and the
/// runtime's own `Add` arms (`value_ops::binary_op`) already stringify a
/// `string`/`int`-or-`float` pair unconditionally. `docs/typed-mode-spec.md`
/// §4 now rules `string + int`/`string + float` (either operand order) as
/// display-concat, typing to `string`; `infer_infix`
/// (`brink-analyzer/src/infer/body.rs`) carries the rule. `for-k-v` no
/// longer produces any strict finding — it is no longer in `BASELINE`.
///
/// **Group E — genuinely unconstrained bindings (`array-literal`,
/// `as-binding`).** **Expected**, and specified: §5's empty-literal rule
/// says an unconstrained `[]` is an `Unknown` escape that the writer
/// annotates, which is exactly `empty_count`'s `let items = [];` (and `x`,
/// its loop binding). `absent`'s `if none as n` binds the payload of a
/// literal `none`, which has no payload type to bind. Both are fixtures
/// written in gradual style, not compiler defects.
///
/// **Group F — unannotated fn-value plumbing (`fn-value-bare-name`).**
/// **Expected**, same reason as `bump`/`heal`: `add(acc, x)` and
/// `apply(g, v)` are unannotated helpers whose bodies do not constrain
/// their parameters, and call-site inference is forbidden by §2.
const BASELINE: &[(&str, &str, &str)] = &[
    // Group A
    (
        "annotations-element",
        "E063",
        "argument 1 of call to `interior` has type `string` but its known type expects `content`",
    ),
    (
        "annotations-element",
        "E063",
        "argument 1 of call to `interior` has type `string` but its known type expects `content`",
    ),
    (
        "annotations-element",
        "E063",
        "argument 2 of call to `radio` has type `string` but its known type expects `content`",
    ),
    (
        "annotations-element",
        "E065",
        "`radio`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    // Group E (array-literal)
    (
        "array-literal",
        "E065",
        "`empty_count`'s temp `items` escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "array-literal",
        "E065",
        "`empty_count`'s temp `x` escapes strict inference as Unknown — annotate or restructure",
    ),
    // Group E (as-binding)
    (
        "as-binding",
        "E065",
        "`absent`'s temp `n` escapes strict inference as Unknown — annotate or restructure",
    ),
    // Group F + Group C (fn-value-bare-name)
    (
        "fn-value-bare-name",
        "E065",
        "`add`'s parameter `acc` escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "fn-value-bare-name",
        "E065",
        "`add`'s parameter `x` escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "fn-value-bare-name",
        "E065",
        "`add`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "fn-value-bare-name",
        "E065",
        "`apply`'s parameter `g` escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "fn-value-bare-name",
        "E065",
        "`apply`'s parameter `v` escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "fn-value-bare-name",
        "E065",
        "`apply`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "fn-value-bare-name",
        "E065",
        "`g` is called as a function value but its type escapes strict inference as Unknown \
         — annotate (`fn(T…): R`) or restructure",
    ),
    (
        "fn-value-bare-name",
        "E065",
        "`mixed`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    // Group D (for-k-v) — issue #1911, FIXED: string+int/string+float
    // display-concat no longer marks `total` Conflicted, so `for-k-v`
    // produces no strict findings at all and has no rows here.
    // Group C (lambda-verbs)
    (
        "lambda-verbs",
        "E065",
        "`braced`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`call_through_capture`'s return type escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`call_through_capture`'s temp `inc` escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`call_through_capture`'s temp `twice` escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`chained`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`doubled`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`field_through_capture`'s return type escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`field_through_capture`'s temp `f` escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`map_each_scaled`'s return type escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`positives`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`scaled`'s parameter `factor` escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`scaled`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`total`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`ufcs_through_capture`'s return type escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "lambda-verbs",
        "E065",
        "`ufcs_through_capture`'s temp `f` escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    // Group B
    (
        "ufcs",
        "E065",
        "`bump`'s parameter `amount` escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "ufcs",
        "E065",
        "`bump`'s parameter `n` escapes strict inference as Unknown — annotate or restructure",
    ),
    (
        "ufcs",
        "E065",
        "`describe_double`'s return type escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "ufcs",
        "E065",
        "`heal`'s parameter `amount` escapes strict inference as Unknown \
         — annotate or restructure",
    ),
    (
        "ufcs",
        "E065",
        "`tally`'s return type escapes strict inference as Unknown — annotate or restructure",
    ),
];

/// The analysis posture a real `.brink` project with `dialect = "brink"` in
/// its `brink.toml` gets.
///
/// `Dialect::Brink` alone already resolves to `TypePolicy::Strict` through
/// issue #1127's dialect-keyed default (verified: dropping the `types` line
/// here leaves all three tests passing). `types` is set explicitly anyway so
/// this sweep pins its own posture rather than inheriting whatever that
/// default becomes later — if the dialect-keyed default ever moves, this
/// file keeps sweeping under strict and
/// [`the_sweep_actually_runs_under_strict`] keeps proving it, instead of
/// quietly going vacuous.
///
/// `dialect` is set alongside it because `types = strict` requires
/// `Dialect::Brink` for `.ink` sources; native compiles skip that `E064`
/// config check entirely (issue #1348), so for this corpus the field is
/// inert beyond supplying the default above.
fn strict_options() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    }
}

/// Every case directory under `tests/tier1-native/`, sorted. Walked rather
/// than listed, so a newly-added corpus case is strict-swept the moment it
/// lands — the `known`-list drift `tier1_native.rs`'s
/// `every_case_directory_has_a_test` guards against cannot happen here.
fn case_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(corpus_dir())
        .expect("read tests/tier1-native")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Compile every corpus case under [`strict_options`] and return the
/// findings as sorted `(case, code, message)` triples.
///
/// Both compile outcomes contribute: a case that fails to compile reports
/// through `CompileError::Diagnostics`, and a case that compiles reports any
/// down-leveled strict diagnostics through `CompileOutput::warnings`. Only
/// the strict-pass codes are collected — a parse or lowering diagnostic
/// unrelated to typing is `tier1_native.rs`'s business, not this file's, and
/// including it here would make this test fail for reasons it cannot triage.
fn strict_findings() -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for name in case_names() {
        let path = corpus_dir().join(&name).join("story.brink");
        let diagnostics = match brink_compiler::compile_path_with_options(&path, strict_options()) {
            Ok(output) => output.warnings,
            Err(CompileError::Diagnostics(ds)) => ds,
            Err(e) => panic!("case {name}: unexpected compile failure: {e}"),
        };
        for d in diagnostics {
            if is_strict_code(d.code) {
                found.push((name.clone(), d.code.as_str().to_string(), d.message));
            }
        }
    }
    found.sort();
    found
}

/// The TM-3 strict pass's own diagnostic codes (`brink_analyzer::strict`):
/// `E063` annotation-vs-inference mismatch, `E065` `Unknown` escape, `E066`
/// `Conflicted` escape — plus `E064` (the `types = strict` requires
/// `dialect = brink` config error). Issue #1348 skips `E064` for native
/// sources, so it is a no-op here today, but it stays in this set so that if
/// it ever *did* fire, `BASELINE` would catch it as a new, unrecognized
/// finding rather than this function silently filtering it away — which is
/// what the previous stringly `"E063" | "E065" | "E066"` match actually did.
fn is_strict_code(code: DiagnosticCode) -> bool {
    matches!(
        code,
        DiagnosticCode::E063 | DiagnosticCode::E064 | DiagnosticCode::E065 | DiagnosticCode::E066
    )
}

/// Render findings as a copy-pasteable list, so a triager reading a failure
/// can see the whole delta without re-running anything.
fn render(findings: &[(String, String, String)]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for (case, code, message) in findings {
        let _ = writeln!(out, "  {case} {code} {message}");
    }
    out
}

/// The gate: the corpus's strict findings must match [`BASELINE`] exactly.
///
/// A failure in either direction is a real signal and neither direction may
/// be resolved by editing the corpus to suit the checker (CLAUDE.md):
///
/// - **new finding** — either a fixture regressed, or a newly-added fixture
///   trips a checker gap. Triage which, then either fix the checker or add
///   the row to `BASELINE` with its classification and tracking issue.
/// - **missing finding** — a checker gap got fixed (celebrate, then delete
///   the row) or a diagnostic silently stopped firing (a regression in the
///   strict pass itself, which is exactly what this direction exists to
///   catch).
#[test]
fn strict_findings_match_recorded_baseline() {
    let actual = strict_findings();
    let expected: Vec<(String, String, String)> = BASELINE
        .iter()
        .map(|(c, k, m)| ((*c).to_string(), (*k).to_string(), (*m).to_string()))
        .collect();
    assert!(
        expected.windows(2).all(|w| w[0] <= w[1]),
        "BASELINE must stay sorted so a diff against `strict_findings` is readable"
    );

    // `new`/`gone` are set-difference only (via `Vec::contains`), so they
    // exist purely to render a readable delta in the failure message below —
    // a finding whose count shifts (e.g. 2 -> 1) while another's shifts the
    // other way (1 -> 2) would leave both `new` and `gone` empty even though
    // the corpus changed. `actual` and `expected` are both sorted, so the
    // `assert_eq!` below is the authoritative, multiset-exact check.
    let new: Vec<_> = actual
        .iter()
        .filter(|f| !expected.contains(f))
        .cloned()
        .collect();
    let gone: Vec<_> = expected
        .iter()
        .filter(|f| !actual.contains(f))
        .cloned()
        .collect();
    assert_eq!(
        actual,
        expected,
        "tier1-native's strict findings drifted from the recorded baseline.\n\
         Do NOT edit the corpus to make this pass — triage each row and either \
         fix the checker or update BASELINE with a classification.\n\
         --- new findings ---\n{}--- findings that stopped firing ---\n{}",
        render(&new),
        render(&gone)
    );
}

/// Guard against the whole file going vacuous. If `strict_options` ever
/// stopped resolving to `TypePolicy::Strict` — a changed dialect-keyed
/// default, a reworked `AnalysisOptions` — `strict::check` would silently
/// stop running and `strict_findings` would return an empty list that
/// matched an emptied baseline without anyone noticing. Both halves are
/// asserted: the policy itself, and that the sweep genuinely observes the
/// strict pass firing.
#[test]
fn the_sweep_actually_runs_under_strict() {
    assert_eq!(
        strict_options().type_policy(),
        TypePolicy::Strict,
        "this file's whole premise is that the corpus is compiled under strict types"
    );
    assert!(
        !BASELINE.is_empty(),
        "an empty baseline means either every gap is fixed (delete this assert and \
         flip the test to `must be clean`) or the sweep stopped observing strict"
    );
    assert!(
        !strict_findings().is_empty(),
        "the strict pass produced nothing at all over the native corpus — it is \
         almost certainly not running (issue #1882's original bug), not that the \
         corpus became clean"
    );
}

/// The premise this file was filed on (issue #1882): the corpus's *default*
/// compile — the one `tier1_native.rs` performs — does not run the strict
/// pass at all. Pinned so that if the native default ever flips to strict,
/// this test fails and whoever flipped it discovers that `tier1_native.rs`'s
/// goldens are now strict-gated too.
#[test]
fn the_default_native_posture_is_still_gradual() {
    assert_eq!(
        AnalysisOptions::default().type_policy(),
        TypePolicy::Gradual,
        "tier1_native.rs compiles via `compile_path` (default options); if that \
         posture is now strict, its goldens run the strict pass and this file's \
         separate sweep needs revisiting"
    );
}
