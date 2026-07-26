//! Tier-1 **native** golden corpus (issue #1529).
//!
//! ⚠ **These goldens are self-referential, NOT oracle-derived.** Native
//! (`.brink`) source has no C# ink counterpart — inklecate never parsed
//! this syntax, so there is nothing for it to generate golden episodes
//! from. Each case under `tests/tier1-native/<name>/` is `story.brink`
//! (the native surface grammar) plus a hand-authored `expected.txt`
//! derived from tracing the case's own semantics against the shipped
//! native pipeline (parser → `hir::lower_native` → analyzer → LIR →
//! codegen → `brink_runtime::Story`), not against the C# runtime. A
//! passing case here proves "the compiler still produces what we
//! previously decided it should produce for this native construct," never
//! "this matches ink's reference behavior" — keep that distinction in any
//! report or doc that cites this corpus. The oracle ratchet
//! (`RATCHET_EPISODE_COUNT` in `oracle_snapshots.rs`) is a completely
//! separate number and must never be conflated with pass/fail counts here.
//!
//! Before this corpus, every native e2e proof lived only as ad-hoc
//! `#[test]` assertions in `crates/brink-compiler/tests/driver.rs` (the
//! `native_or_coalescing_*`/`native_as_binding_*` families) and a handful
//! of sibling integration-test files — real coverage, but invisible to
//! `corpus_report` (`cargo test -p brink-test-harness --test corpus_report
//! -- --nocapture`), which only walks `tests/tier{1,2,3}/` and
//! `tests_github/`. This corpus is `corpus_report`'s native counterpart,
//! reported in its own clearly-labeled section (see that test) so a
//! native regression surfaces where everyone already looks, without
//! polluting the oracle's CASES/EPISODES totals.
//!
//! Seeded (2026-07-26) with the native features that shipped this same
//! week: or-coalescing (issue #1460, short-circuit ruling #1471),
//! the `as` binding (issue #1475), UFCS calls — free-fn/auto-ref/prelude
//! desugar (issue #1482), `TypeName { … }` construction literals
//! (issue #1464), two-binding `for k, v in` map iteration (issue #1461),
//! and the `@[was(…)]` module-rename annotation (issue #1286/#1355).
//! Complements, never replaces, `driver.rs`'s fine-grained unit-level
//! assertions on the same features.
//!
//! Mirrors `tier1_brink.rs`'s shape (that file's own module doc explains
//! why a hand-curated golden corpus is the right tool when there is no
//! oracle to diff against). Every case is a straight-line, choice-free
//! program — `compile_path` auto-detects the `.brink` extension and
//! routes through the native pipeline with default `AnalysisOptions`
//! (no `Dialect`/`TypePolicy` knob native cares about).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-native")
}

fn assert_case(name: &str) {
    let dir = corpus_dir().join(name);
    let expected =
        brink_test_harness::corpus::load_golden_transcript(&dir.join("expected.txt"), name)
            .expect("golden transcript must be present and non-vacuous");
    let actual = brink_test_harness::corpus::run_native_transcript(&dir.join("story.brink"))
        .unwrap_or_else(|e| panic!("case {name}: {e}"));
    assert_eq!(
        actual, expected,
        "case {name}: output mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
    );
}

/// B1 or-coalescing (`docs/stdlib-spec.md` §1.6a, issue #1460): the
/// collapse form (`some(v) or fallback` / `none or fallback`), a chained
/// `none or none or 7`, and the short-circuit ruling (#1471) — `bump()`
/// only runs, and `counter` only advances, when the left-hand side is
/// `none`.
#[test]
fn or_coalescing() {
    assert_case("or-coalescing");
}

/// B1b the `as` binding (issue #1475): `if EXPR as NAME { … }` binds the
/// *unwrapped* payload inside the success arm and falls through to the
/// story's own fallback when the condition is `none`.
#[test]
fn as_binding() {
    assert_case("as-binding");
}

/// B3a UFCS resolution (issue #1482): one story exercising all three
/// desugar verdicts — `hp.bump(amount)` with `bump`'s `ref` first
/// parameter (`FreeFnAutoRef`, mutates the global through the call),
/// `n.double()` against a plain by-value free function
/// (`FreeFnDesugar`), and `m.len()` against the T1b/NS stdlib prelude
/// (`PreludeDesugar`).
#[test]
fn ufcs_desugar_forms() {
    assert_case("ufcs");
}

/// B5 construction literals (issue #1464): `TypeName { field: value, … }`
/// against an unregistered type name falls through to the declared-struct
/// reading (`Expr::StructLiteral`) — construct, read, and use a field
/// value end to end.
#[test]
fn construction_literal() {
    assert_case("construction-literal");
}

/// B2 two-binding `for k, v in m` (issue #1461): desugars to a container
/// snapshot plus a `LogicWhile` binding `k`/`v` each iteration — summing
/// every value in a three-entry map proves the loop actually runs to
/// completion, not just that it lowers.
#[test]
fn for_k_v() {
    assert_case("for-k-v");
}

/// The `@[was("old::path")]` file-level module-rename annotation
/// (issue #1286/#1355) parses and lowers cleanly on a real compile —
/// the shipped slice of the native annotation grammar (per-declaration
/// `@[…]` directives are still unwired and are *not* claimed here; see
/// `brink_ir::hir::lower_native`'s module doc, judgment call #5). The
/// annotation's full DefinitionId-continuity-across-a-rename semantics
/// are a project/db-layer concern covered by `brink-ir`/`brink-db`'s own
/// tests, not by this single-compile transcript corpus — this case's
/// signal is narrower: the annotation must never regress a clean compile.
#[test]
fn annotations_was() {
    assert_case("annotations-was");
}

/// Every `tests/tier1-native/` case directory is exercised by a `#[test]`
/// above — a directory with no matching test would silently never run.
#[test]
fn every_case_directory_has_a_test() {
    let known = [
        "or-coalescing",
        "as-binding",
        "ufcs",
        "construction-literal",
        "for-k-v",
        "annotations-was",
    ];
    let mut found: Vec<String> = std::fs::read_dir(corpus_dir())
        .expect("read tests/tier1-native")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut expected: Vec<String> = known.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "tests/tier1-native/ directories and this test's `known` list have drifted"
    );
}
