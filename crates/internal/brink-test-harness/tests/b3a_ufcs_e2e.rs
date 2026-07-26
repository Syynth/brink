//! B3a end-to-end: UFCS resolution on the native surface, through the real
//! `.brink` pipeline (issue #1482; D1–D5 RULED 2026-07-26 —
//! `docs/decision-log.md` "UFCS resolution pass designed").
//!
//! The unit tests in `brink-analyzer/tests/ufcs_resolution.rs` prove the
//! *pass* (verdicts + the four diagnostic outcomes). This file proves the
//! **user path**: a real `.brink` source reaching
//! `brink_test_harness::corpus::explore_from_brink_native` — the same
//! honest minimal native pipeline `b5_construction_e2e.rs` and
//! `first_light.rs` run — gets each ruled outcome as a compile refusal,
//! with the *right* code.
//!
//! Every case here is a refusal, and that is the point of the file rather
//! than a shortfall: before this pass every UFCS-shaped call was refused
//! indiscriminately as `E025` ("unresolved variable reference"), and the
//! bar this pass has to clear is that each site now fails — or succeeds —
//! *for its own ruled reason*. A **resolved** call reaches `E144` at LIR
//! lowering (`brink-ir::lir::lower::expr::lower_call`), which does not
//! consume the D2 verdict side table yet; the E144 case below is the guard
//! that the resolution never silently lowers into a call against the
//! receiver's own id.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::explore_from_brink_native;

/// Compile-and-play, expecting refusal; returns the rendered error.
fn refuse(src: &str) -> String {
    match explore_from_brink_native(src, &ExploreConfig::default()) {
        Ok(_) => panic!("fixture must be refused, but it compiled"),
        Err(e) => e,
    }
}

/// A `.brink` file whose `shout()` body is `BODY`.
fn program(decls: &str, body: &str) -> String {
    format!(
        "\
struct Guest {{
  name: string
}}

{decls}
flow main() {{
  {{shout()}}
}}

fn shout() {{
  let g = Guest {{ name: \"ada\" }};
  {body}
}}
"
    )
}

const GREET: &str = "\
fn greet(g, loudness) {
  return loudness;
}
";

/// A resolvable method call no longer fails as an unresolved reference —
/// it gets all the way to LIR lowering and stops there, loudly (`E144`),
/// because the verdict side table has no consumer yet. The pre-#1482
/// `E025` must be gone: that is the observable proof the resolution ran.
#[test]
fn a_resolved_method_call_reaches_lowering_and_is_refused_there_not_as_e025() {
    let err = refuse(&program(GREET, "return g.greet(3);"));
    assert!(err.contains("E144"), "expected E144, got: {err}");
    assert!(
        !err.contains("E025"),
        "the unresolved-reference refusal must be gone: {err}"
    );
}

/// D1: a matching but non-callable field is a hard error, and it never
/// falls through to the free `greet` that is also in scope.
#[test]
fn a_non_callable_field_refuses_the_compile_with_e140() {
    let src = format!(
        "\
struct Guest {{
  greet: string
}}

{GREET}
flow main() {{
  {{shout()}}
}}

fn shout() {{
  let g = Guest {{ greet: \"hi\" }};
  return g.greet(3);
}}
"
    );
    let err = refuse(&src);
    assert!(err.contains("E140"), "expected E140, got: {err}");
    assert!(
        !err.contains("E141"),
        "field access wins outright — no second attempt is reported: {err}"
    );
}

/// Step 4: neither a field nor a free function.
#[test]
fn an_unresolvable_method_refuses_the_compile_with_e141() {
    let err = refuse(&program("", "return g.nope(3);"));
    assert!(err.contains("E141"), "expected E141, got: {err}");
}

/// D3: an unknown receiver type demands an annotation instead of deferring.
#[test]
fn an_unknown_receiver_refuses_the_compile_with_e142() {
    let src = format!(
        "\
{GREET}
flow main() {{
  {{shout(1)}}
}}

fn shout(guest) {{
  return guest.greet(3);
}}
"
    );
    let err = refuse(&src);
    assert!(err.contains("E142"), "expected E142, got: {err}");
}

/// D5 fence: auto-ref is #1462. A `ref` first parameter is refused rather
/// than desugared by value, which would silently drop the mutation.
#[test]
fn a_ref_first_param_refuses_the_compile_with_e143() {
    let err = refuse(&program(
        "\
fn heal(ref g, amount) {
  return amount;
}
",
        "return g.heal(5);",
    ));
    assert!(err.contains("E143"), "expected E143, got: {err}");
    assert!(err.contains("#1462"), "must point at the follow-up: {err}");
}

/// The explicit free-call spelling keeps working end to end — the sugar is
/// additive, and this is the workaround every refusal above points authors
/// at.
#[test]
fn the_explicit_free_call_spelling_still_plays() {
    let src = format!(
        "\
struct Guest {{
  name: string
}}

{GREET}
flow main() {{
  Loudness is {{shout()}}.
}}

fn shout() {{
  let g = Guest {{ name: \"ada\" }};
  return greet(g, 3);
}}
"
    );
    let episodes = explore_from_brink_native(&src, &ExploreConfig::default())
        .unwrap_or_else(|e| panic!("native fixture must compile and play: {e}"));
    let out: String = episodes
        .first()
        .expect("one episode")
        .steps
        .iter()
        .map(|s| s.text.clone())
        .collect();
    assert_eq!(out, "Loudness is 3.\n");
}
