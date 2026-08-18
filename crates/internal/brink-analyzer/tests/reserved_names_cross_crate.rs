#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Issue #2863: a mechanical cross-crate check that the reserved-name
//! predicates production code actually calls agree with each other.
//!
//! ## What this issue investigated
//!
//! Before this PR, `brink_analyzer::resolve::is_builtin_function`/
//! `is_t1b_stdlib_name` and `brink_ir::lir::lower::expr::recognize_builtin`/
//! `is_t1b_stdlib_name` were four independently hand-maintained `matches!`
//! lists spread across two crates, each carrying a doc comment admitting it
//! drifts by hand against its counterpart. `brink-ide`'s
//! `crates/internal/brink-ide/src/stdlib.rs` looked like a fifth copy at
//! first glance, but turned out on inspection to be a genuinely different,
//! smaller table — slice-1-only names with richer hover/completion metadata
//! (params, lvalue-ness, return type, doc text), not the full T1b stdlib
//! surface — so it is handled separately, by a subset test in that crate
//! plus a corrected doc comment, not folded into the equality check here.
//!
//! `brink_analyzer::dialect_gate::is_t1b_stdlib_call_name` is not a
//! duplicate at all — it was already a one-line delegate to
//! `resolve::is_t1b_stdlib_name`, just with a doc comment that read like a
//! hand-kept copy (fixed in this PR).
//!
//! This PR **unified** the two full-surface lists rather than merely
//! testing for drift between them: `brink_ir::lir::is_builtin_function`/
//! `is_t1b_stdlib_name` are now the single canonical implementation (this
//! crate already depends on `brink-ir` — the edge only runs one way, so
//! `brink-ir` can't depend back — making it the natural home), and
//! `brink_analyzer::resolve::is_builtin_function`/`is_t1b_stdlib_name` are
//! now thin delegates to them. That makes the two crates' predicates
//! trivially, structurally equal — there is only one `matches!` list left
//! for the "full surface" name sets, not two that happen to agree today.
//!
//! ## Why this test still exists
//!
//! A trivial-by-construction equality is not a permanent guarantee: nothing
//! stops a future edit from reintroducing a hand-written `matches!` in
//! `resolve.rs` instead of calling through to `brink_ir::lir`. This test
//! calls both crates' public predicates directly (not through any shared
//! helper) across a large, randomly generated set of candidate names, so a
//! future re-divergence — in either direction, for either name set — fails
//! CI here instead of shipping silently.
//!
//! ## What this test deliberately does NOT claim to prove
//!
//! Content equality is **necessary but not sufficient**. PR #2859 (the
//! issue that motivated filing #2863) is the proof: its two reserved-name
//! lists agreed on content — 22 names each, confirmed by the reviewer —
//! and a real silent-drop bug still reached production, because what had
//! drifted was the surrounding *resolution order*: `brink-ir`'s LIR-lowering
//! copy consulted the builtin table before the analyzer's resolution map,
//! so a correctly resolved, correctly shadowing knot named `FLOOR` was
//! silently discarded in favor of the real `FLOOR()` builtin at its own
//! call site. A pure content-equality test — this file — would not have
//! caught that bug and does not catch its class of bug now. The order
//! invariant is a separate, per-call-site property, pinned end-to-end
//! (compile + run, not just resolve) by
//! `crates/brink-compiler/tests/issue_2856_builtin_shadow.rs`. Both tests
//! are required; neither subsumes the other.

use brink_analyzer::test_support::{
    is_builtin_function as analyzer_is_builtin_function,
    is_t1b_stdlib_name as analyzer_is_t1b_stdlib_name,
};
use proptest::prelude::*;

/// A handful of known-reserved names, spot-checked directly (not exhaustive
/// — see this file's module doc for why an exhaustive parallel list is
/// exactly the anti-pattern #2863 is about). These pin the two crates'
/// predicates to the *same concrete answer* for names drawn from both
/// reserved-name families, not just "both return some boolean."
const KNOWN_BUILTIN_NAMES: &[&str] = &["RANDOM", "FLOOR", "TURNS_SINCE", "LIST_COUNT", "MAX"];
const KNOWN_T1B_STDLIB_NAMES: &[&str] = &["push", "len", "map", "sort_by", "vec2", "weighted"];
const KNOWN_NON_RESERVED_NAMES: &[&str] = &[
    "gold",
    "player_name",
    "MyKnot",
    "quest_state",
    "totally_ordinary",
];

#[test]
fn known_builtin_names_agree_across_crates() {
    for name in KNOWN_BUILTIN_NAMES {
        assert!(
            analyzer_is_builtin_function(name),
            "brink_analyzer::resolve::is_builtin_function should recognize `{name}`"
        );
        assert!(
            brink_ir::lir::is_builtin_function(name),
            "brink_ir::lir::is_builtin_function should recognize `{name}`"
        );
    }
}

#[test]
fn known_t1b_stdlib_names_agree_across_crates() {
    for name in KNOWN_T1B_STDLIB_NAMES {
        assert!(
            analyzer_is_t1b_stdlib_name(name),
            "brink_analyzer::resolve::is_t1b_stdlib_name should recognize `{name}`"
        );
        assert!(
            brink_ir::lir::is_t1b_stdlib_name(name),
            "brink_ir::lir::is_t1b_stdlib_name should recognize `{name}`"
        );
    }
}

#[test]
fn known_non_reserved_names_agree_across_crates() {
    for name in KNOWN_NON_RESERVED_NAMES {
        assert!(
            !analyzer_is_builtin_function(name) && !brink_ir::lir::is_builtin_function(name),
            "`{name}` should not be a classic builtin in either crate"
        );
        assert!(
            !analyzer_is_t1b_stdlib_name(name) && !brink_ir::lir::is_t1b_stdlib_name(name),
            "`{name}` should not be a T1b stdlib name in either crate"
        );
    }
}

proptest! {
    /// The property-based half: across a large, randomly generated set of
    /// candidate names (both charsets the two real predicates actually
    /// discriminate on — all-uppercase-with-underscores for the classic
    /// builtins, all-lowercase-with-underscores for the T1b stdlib names —
    /// plus mixed-case names that should be reserved in neither crate),
    /// `brink_analyzer`'s and `brink_ir`'s predicates must return the exact
    /// same verdict for every single one. This is the check that actually
    /// scales past the handful of names spot-checked above, and the one
    /// that would have failed CI immediately had this PR's delegation been
    /// implemented backwards (e.g. `brink_ir` calling into `brink_analyzer`,
    /// which would not even compile given the dependency direction, or a
    /// stray hand-copy left behind in either crate).
    #[test]
    fn is_builtin_function_agrees_across_crates(name in "[A-Z][A-Z_]{0,15}") {
        prop_assert_eq!(
            analyzer_is_builtin_function(&name),
            brink_ir::lir::is_builtin_function(&name),
            "is_builtin_function disagreed between brink_analyzer and brink_ir for `{}`",
            name,
        );
    }

    #[test]
    fn is_t1b_stdlib_name_agrees_across_crates(name in "[a-z][a-z_]{0,15}") {
        prop_assert_eq!(
            analyzer_is_t1b_stdlib_name(&name),
            brink_ir::lir::is_t1b_stdlib_name(&name),
            "is_t1b_stdlib_name disagreed between brink_analyzer and brink_ir for `{}`",
            name,
        );
    }

    /// Mixed-case names should be reserved by neither predicate, in either
    /// crate — a cheap extra corner the pure-uppercase/pure-lowercase
    /// strategies above never generate.
    #[test]
    fn mixed_case_names_are_reserved_nowhere(name in "[A-Za-z][A-Za-z_]{0,15}") {
        let has_upper = name.chars().any(|c| c.is_ascii_uppercase());
        let has_lower = name.chars().any(|c| c.is_ascii_lowercase());
        prop_assume!(has_upper && has_lower);

        prop_assert_eq!(analyzer_is_builtin_function(&name), false);
        prop_assert_eq!(brink_ir::lir::is_builtin_function(&name), false);
        prop_assert_eq!(analyzer_is_t1b_stdlib_name(&name), false);
        prop_assert_eq!(brink_ir::lir::is_t1b_stdlib_name(&name), false);
    }
}
