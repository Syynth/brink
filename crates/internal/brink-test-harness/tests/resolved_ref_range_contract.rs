//! Pins the call-path `ResolvedRef::range` contract (issue #1561).
//!
//! `brink_ir::ResolvedRef::range` for a call (`Expr::Call(path, _)`) must be
//! **exactly `path.range`** — the callee `Path`'s own whole span, never a
//! narrowed sub-segment. That single range is an independent `(FileId,
//! TextRange)` lookup key at least six separate consumers rely on:
//!
//! 1. `brink_ir::lir::lower::expr::lower_call`'s `ctx.resolve_path(path.range)`;
//! 2. `brink_ir::lir::lower::expr::ufcs_receiver_path`, which deliberately
//!    keeps `path.range` on the desugared receiver sub-path so lookup 1
//!    still hits for the receiver too;
//! 3. `brink_analyzer::strict`'s `E067` void-assignment check
//!    (`check_void_root`'s `resolution_by_range` lookup);
//! 4. `brink_analyzer::coalesce`'s operand classifier (`classify_coalesce_
//!    operand`'s `resolution_by_range` lookup);
//! 5. `brink_analyzer::ufcs::value_receiver_def`'s `resolution_by_range`
//!    lookup on the callee path — the mirror of `resolve::resolve_function`'s
//!    own UFCS-shaped fallback, which must agree with it or a call is
//!    diagnosed twice or not at all; and
//! 6. `brink_analyzer::infer::body::infer_call`'s `self.resolve(path.range)`
//!    (backed by the same `resolution_by_range` map), whose B3a branch
//!    explicitly handles a multi-segment (dotted UFCS) callee path.
//!
//! Before this test existed, nothing pinned the production range as a
//! reusable, named contract — this file makes the range itself, and every
//! consumer above, an explicit regression surface rather than relying on
//! whichever end-to-end fixture happens to exercise a given call shape. The
//! bug class it targets (#1539/#1550/#1560) was caught only by an alert
//! reviewer for #1550's build, not by any test at the time. This file pins
//! the production range directly, then proves it round-trips through three
//! of the six consumers above: `strict`'s void-use check, and — over a
//! **dotted-chain** receiver (`o.inner.n.bump(…)`, two field segments deep,
//! deliberately distinct from the single-segment shape
//! `brink-test-harness/tests/b3a_ufcs_e2e.rs`'s
//! `auto_ref_mutates_a_local_receiver_end_to_end` already covers) —
//! `lower_call`'s and `ufcs_receiver_path`'s independent lookups, via their
//! real, unmodified code paths.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ops::Range;

use brink_analyzer::{AnalysisOptions, Dialect, ImportScope, TypePolicy};
use brink_ir::FileId;
use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::explore_from_brink_native;

/// Resolve a native `.brink` fixture through the real
/// `brink-analyzer::resolve` pass, returning the source alongside its
/// `ResolutionMap` for direct range inspection.
fn resolve_native(src: &str) -> brink_ir::ResolutionMap {
    let parse = brink_syntax_native::parse(src);
    assert!(
        parse.errors().is_empty(),
        "fixture must parse cleanly: {:?}",
        parse.errors()
    );
    let (_hir, manifest, _diag) = brink_ir::hir::lower_native::lower(FileId(0), &parse.tree());
    let (index, _diag) = brink_analyzer::symbol_index(&[(FileId(0), &manifest)]);
    let (resolutions, _diag) =
        brink_analyzer::resolve(FileId(0), &manifest, &index, &ImportScope::default());
    (*resolutions).clone()
}

/// The literal source text a `ResolvedRef` spans, for direct range
/// assertions instead of comparing opaque `TextRange` values.
fn ref_texts<'a>(src: &'a str, resolutions: &brink_ir::ResolutionMap) -> Vec<&'a str> {
    resolutions
        .iter()
        .map(|r| &src[Range::<usize>::from(r.range)])
        .collect()
}

/// No standalone reference to `g` anywhere but inside the call path itself
/// — so a `"g"` entry in the resolution map can only mean the call-path
/// range narrowed to the receiver segment alone, never a legitimate
/// unrelated variable reference.
const UFCS_PIN_SRC: &str = "\
fn bump(ref n, amount) {
  n = n + amount;
}

fn total() {
  let g = 1;
  g.bump(5);
}
";

/// A **dotted-chain** receiver (`o.inner.n`, two field segments deep) —
/// deliberately distinct from [`UFCS_PIN_SRC`]/[`the_pinned_whole_path_
/// range_round_trips_through_lower_call_and_ufcs_receiver_path`]'s
/// single-segment receiver (`g`), and from `b3a_ufcs_e2e.rs`'s
/// `auto_ref_mutates_a_local_receiver_end_to_end` (same single-segment
/// shape). `ufcs_receiver_path`'s own doc calls out that it must resolve
/// correctly "whether the receiver is one segment (`x`) or a dotted chain
/// (`a.b`)" — this fixture is the dotted-chain half of that claim, proven
/// end-to-end rather than only at the unit level (`brink-ir/tests/
/// ufcs_auto_ref.rs`'s `a_dotted_receiver_auto_refs_as_an_explicit_
/// projection`, which supplies its verdict by hand rather than through the
/// real analyzer).
const UFCS_DOTTED_SRC: &str = "\
struct Inner {
  n: int
}

struct Outer {
  inner: Inner
}

var o: Outer = 0

fn bump(ref x, amount) {
  x = x + amount;
}

fn total() {
  o = Outer { inner: Inner { n: 1 } };
  o.inner.n.bump(5);
  return o.inner.n;
}

flow main() {
  Total is {total()}.
}
";

/// Pins production: `resolve_function`'s B3a UFCS branch records the call
/// site's `ResolvedRef` against `g.bump`'s **whole** dotted span — never
/// narrowed to just the receiver (`g`) or just the method segment
/// (`bump`). This is the exact site #1550's build risked narrowing.
#[test]
fn ufcs_call_path_resolves_to_the_whole_path_span_not_a_sub_segment() {
    let resolutions = resolve_native(UFCS_PIN_SRC);
    let texts = ref_texts(UFCS_PIN_SRC, &resolutions);

    assert!(
        texts.contains(&"g.bump"),
        "expected a ResolvedRef spanning the whole call path `g.bump` — found: {texts:?}"
    );
    assert!(
        !texts.contains(&"g"),
        "the call-path range must never narrow to the receiver segment alone: {texts:?}"
    );
    assert!(
        !texts.contains(&"bump"),
        "the call-path range must never narrow to the method segment alone: {texts:?}"
    );
}

/// Round-trips that exact whole-path range through two independent
/// consumers at once, over a **dotted-chain** receiver: `lower_call`'s own
/// `ctx.resolve_path(path.range)` (which must resolve `o.inner.n.bump`'s
/// range to the durable root `o`, taking the B3a UFCS branch) and
/// `ufcs_receiver_path` (which must then reuse the same range so lowering
/// the desugared receiver argument resolves it a second time, correctly, as
/// a two-segment `ref` projection — `inner`, then `n` — onto the *same*
/// global `o`).
///
/// If either consumer's key ever drifted from the analyzer's whole-path
/// range — independently, since they are two different lookups against the
/// same map — this auto-ref mutation would either fail to compile (`E144`,
/// the "no verdict" fallback) or silently mutate nothing, and the assertion
/// below would catch it either way.
#[test]
fn the_pinned_whole_path_range_round_trips_through_lower_call_and_ufcs_receiver_path() {
    let episodes = explore_from_brink_native(UFCS_DOTTED_SRC, &ExploreConfig::default())
        .unwrap_or_else(|e| panic!("UFCS auto-ref fixture must compile and play: {e}"));
    let out: String = episodes
        .first()
        .expect("one episode")
        .steps
        .iter()
        .map(|s| s.text.clone())
        .collect();
    assert_eq!(
        out, "Total is 6.\n",
        "o.inner.n.bump(5)'s mutation must be visible in `o.inner.n` afterwards — only \
         possible if lower_call and ufcs_receiver_path both resolved the identical \
         whole-path range, over a two-segment dotted-chain receiver"
    );
}

/// The third consumer, in a different crate: `strict::check_void_root`'s
/// `resolution_by_range` lookup keys on the exact same call-path range this
/// file's first test pinned. A plain (non-UFCS) void call is enough to
/// exercise it — `check_void_root` only ever inspects a call's *own*
/// resolved def, and a UFCS call's whole-path range resolves to its
/// receiver (never a void knot), so this consumer's real trigger case is
/// the ordinary single-segment call the void-assignment rule (`E067`,
/// `docs/typed-mode-spec.md` §3) is about.
#[test]
fn the_same_call_path_range_contract_is_what_strict_void_use_keys_on() {
    let src = "\
fn noop(): void {
}

fn total() {
  let x = noop();
  return x;
}

flow main() {
  {total()}
}
";
    let resolutions = resolve_native(src);
    let texts = ref_texts(src, &resolutions);
    assert!(
        texts.contains(&"noop"),
        "expected a ResolvedRef spanning the whole call path `noop` — found: {texts:?}"
    );

    let parse = brink_syntax_native::parse(src);
    let (hir, manifest, _diag) = brink_ir::hir::lower_native::lower(FileId(0), &parse.tree());
    let opts = AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    };
    let result = brink_analyzer::analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == brink_ir::DiagnosticCode::E067),
        "expected E067 (void-assignment) — `strict::check_void_root` must have found the \
         same call-path range this test pinned above; diagnostics: {:?}",
        result.diagnostics
    );
}
