//! Field-mutator (`push(a.items, v)`/`insert`/`remove_at`/…) take/`RecordSet`
//! RMW-discipline tests (issue #2123, `docs/value-model-spec.md` §5).
//!
//! `take_rmw.rs` covers the *root-level* `TakeGlobal`/`TakeTemp` cliff (issue
//! #576) and its fault-during-RMW slot-state property for a bare variable.
//! Issue #1495 (PR #2106) then split a struct-field-projection mutator
//! (`push(a.items, v)`) off `lower_bare_mutator` into its own
//! `lower_field_mutator`, but that fix still read the field via a *cloning*
//! `RecordGet`, leaving the field's `Arc` doubly referenced by the time the
//! RMW ran — an O(n²) loop-append cliff one field deeper than #576 closed.
//! This file is `take_rmw.rs`'s field-scoped sibling:
//!
//! - RMW-equivalence and the sharing-unobservable law for
//!   `push`/`insert`/`remove_at` on a single-level struct-field projection.
//! - The fault-during-RMW slot-state property `lower_field_mutator`'s own
//!   doc claims: a mid-RMW fault leaves `root_target` a *structurally valid
//!   record* with only the mutated field blown away to `Value::Null` — a
//!   narrower, field-scoped version of `fault_during_insert_leaves_root_null`
//!   et al. (`take_rmw.rs`), never the whole root lost.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, Dialect};
use brink_format::Value;
use brink_runtime::{DotNetRng, RuntimeError, Step, Story};
use proptest::prelude::*;

/// Compile `source` under the brink dialect and return a linked, unstarted
/// `Story`.
fn compile(source: &str) -> Story<DotNetRng> {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let compile_msg = format!("compile error for:\n{source}");
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_string()), options)
            .expect(&compile_msg);
    let link_msg = format!("link error for:\n{source}");
    let (program, line_tables) = brink_runtime::link(&output.data).expect(&link_msg);
    Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables)
}

/// Run `story` to completion (choice-free), returning the concatenated text
/// on success or the `RuntimeError` that terminated the turn. Either way,
/// `story` is left exactly where execution stopped — callers inspect
/// post-fault state via `story.variable(name)`.
fn run_to_completion_or_fault(story: &mut Story<DotNetRng>) -> Result<String, RuntimeError> {
    let mut out = String::new();
    loop {
        match story.continue_single()? {
            Step::Line(line) => out.push_str(&line.text),
            Step::Done | Step::End | Step::Suspended | Step::Choices(_) => return Ok(out),
        }
    }
}

fn space_joined(values: &[i32]) -> String {
    values
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Unwrap a global's `items: Array<int>` field (offset 0 of `Bag`) as a
/// `Vec<i32>`, panicking with a descriptive message otherwise — test-only
/// helper, not the story's own API.
fn bag_items(story: &Story<DotNetRng>, name: &str) -> Vec<i32> {
    let Some(Value::Record { fields, .. }) = story.variable(name) else {
        panic!("{name} is not a declared record");
    };
    let Some(Value::Array(items)) = fields.first() else {
        panic!("{name}.items (field 0) is not an array: {fields:?}");
    };
    items
        .iter()
        .map(|v| match v {
            Value::Int(n) => *n,
            other => panic!("non-int array element: {other:?}"),
        })
        .collect()
}

// ── RMW-equivalence + sharing law ────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// `push(a.items, v)` in a loop matches the manual take/mutate/write-back
    /// a Rust reference performs on an equivalent `Vec<i32>` — the exact
    /// shape issue #2123's repro exercises.
    #[test]
    fn field_push_matches_manual_rmw(
        base in prop::collection::vec(-1000i32..1000, 0..8),
        extra in prop::collection::vec(-1000i32..1000, 0..8),
    ) {
        let mut reference = base.clone();
        reference.extend(extra.iter().copied());

        let literal = format!(
            "#[{}]",
            base.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
        );
        let pushes = extra.iter().fold(String::new(), |mut acc, v| {
            use std::fmt::Write as _;
            let _ = writeln!(acc, "    push(a.items, {v})");
            acc
        });
        let source = format!(
            "STRUCT Bag = #{{\n    items: Array<int>,\n}}\nVAR a = 0\nVAR out = \"\"\n~ {{\n    a = Bag#{{items: {literal}}}\n{pushes}    for x in a.items {{\n        out = out + \" \" + x\n    }}\n}}\n{{out}}\n-> END\n",
        );
        let mut story = compile(&source);
        let out = run_to_completion_or_fault(&mut story).expect("no fault expected");
        prop_assert_eq!(out.trim(), space_joined(&reference));
    }

    /// Sharing-unobservable law (value-model-spec §3) applied to the
    /// field-mutator fast path: `b = a` then `push(b.items, v)` never
    /// changes `a.items`, whether or not `a`'s Arc was uniquely owned going
    /// in — the take/de-alias-based RMW must COW exactly when something
    /// else is still watching.
    #[test]
    fn copy_then_field_push_never_observes_through_the_original(
        base in prop::collection::vec(-1000i32..1000, 0..8),
        v in -1000i32..1000,
    ) {
        let original = base.clone();
        let mut mutated = base.clone();
        mutated.push(v);

        let literal = format!(
            "#[{}]",
            base.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
        );
        let source = format!(
            "STRUCT Bag = #{{\n    items: Array<int>,\n}}\nVAR a = 0\nVAR b = 0\nVAR out_a = \"\"\nVAR out_b = \"\"\n~ {{\n    a = Bag#{{items: {literal}}}\n    b = a\n    push(b.items, {v})\n    for x in a.items {{\n        out_a = out_a + \" \" + x\n    }}\n    for x in b.items {{\n        out_b = out_b + \" \" + x\n    }}\n}}\n{{out_a}}\nSPLIT\n{{out_b}}\n-> END\n",
        );
        let mut story = compile(&source);
        let out = run_to_completion_or_fault(&mut story).expect("no fault expected");
        let mut parts = out.split("SPLIT");
        let a_text = parts.next().unwrap_or_default();
        let b_text = parts.next().unwrap_or_default();
        prop_assert_eq!(a_text.trim(), space_joined(&original));
        prop_assert_eq!(b_text.trim(), space_joined(&mutated));
    }
}

// ── The O(n²) cliff itself (issue #2123's repro, pinned as a regression) ──

/// `push(a.items, i)` in a 2,000-iteration loop must not silently re-share
/// the field's `Arc` across iterations — pinned here as a *value* check
/// (the final length), with the actual COW-copy-count proof living in
/// `crates/brink-runtime/benches/runtime.rs`'s `field_loop_append_bench`
/// (see `docs/runtime-bench.md`).
#[test]
fn field_push_loop_produces_correct_length_at_scale() {
    let source = "STRUCT Bag = #{\n    items: Array<int>,\n}\nVAR a = 0\nVAR total = 0\n~ {\n    a = Bag#{items: #[]}\n    temp i = 0\n    while i < 2000 {\n        push(a.items, i)\n        i = i + 1\n    }\n    total = len(a.items)\n}\n{total}\n-> END\n";
    let mut story = compile(source);
    let out = run_to_completion_or_fault(&mut story).expect("no fault expected");
    assert_eq!(out.trim(), "2000");
    assert_eq!(bag_items(&story, "a").len(), 2000);
}

// ── Fault-during-RMW slot state (issue #2123's field-scoped trade-off) ──

/// `remove_at(a.items, i)` with `i` out of bounds faults — same
/// out-of-range fault `take_rmw.rs`'s `fault_during_remove_at_leaves_root_null`
/// documents for the bare-variable case, but scoped to the field: `a`
/// itself stays a *structurally valid* `Bag` record (never collapsed to a
/// bare `Value::Null`), with only `items` — the field the RMW actually
/// touched — blown away to `Value::Null`. The struct's *other* field
/// (`tag`) is untouched, proving the trade-off really is field-scoped, not
/// a relabeled version of the whole-root trade-off.
#[test]
fn fault_during_field_remove_at_leaves_only_that_field_null() {
    let source = "STRUCT Bag = #{\n    items: Array<int>,\n    tag: int,\n}\nVAR a = 0\n~ {\n    a = Bag#{items: #[1, 2, 3], tag: 7}\n    remove_at(a.items, 99)\n}\n{a.items[0]}\n-> END\n";
    let mut story = compile(source);
    let err = run_to_completion_or_fault(&mut story).expect_err("index 99 is out of bounds");
    assert!(
        matches!(err, RuntimeError::IndexOutOfBounds { index: 99, len: 3 }),
        "unexpected error: {err:?}"
    );
    let Some(Value::Record { fields, .. }) = story.variable("a") else {
        panic!("a is not a declared record after the fault");
    };
    assert_eq!(
        fields.as_slice(),
        &[Value::Null, Value::Int(7)],
        "documented fault-during-RMW slot state for a field mutator: only \
         the mutated field (`items`, offset 0) is Value::Null — `tag` \
         (offset 1) is untouched, and `a` is still a Bag record, never \
         collapsed to a bare Value::Null"
    );
}

/// `insert(a.items, k, v)` with `k` out of bounds — same documented
/// field-scoped `Value::Null` outcome as `remove_at`'s fault case above.
#[test]
fn fault_during_field_insert_leaves_only_that_field_null() {
    let source = "STRUCT Bag = #{\n    items: Array<int>,\n    tag: int,\n}\nVAR a = 0\n~ {\n    a = Bag#{items: #[1, 2, 3], tag: 7}\n    insert(a.items, 99, 5)\n}\n{a.items[0]}\n-> END\n";
    let mut story = compile(source);
    let err = run_to_completion_or_fault(&mut story).expect_err("index 99 is out of bounds");
    assert!(
        matches!(err, RuntimeError::IndexOutOfBounds { index: 99, len: 3 }),
        "unexpected error: {err:?}"
    );
    let Some(Value::Record { fields, .. }) = story.variable("a") else {
        panic!("a is not a declared record after the fault");
    };
    assert_eq!(fields.as_slice(), &[Value::Null, Value::Int(7)]);
}

/// A fault during one struct field's RMW must not corrupt an *unrelated*
/// global, mirroring `take_rmw.rs`'s
/// `fault_during_rmw_does_not_touch_unrelated_globals` for the field case.
#[test]
fn fault_during_field_rmw_does_not_touch_unrelated_globals() {
    let source = "STRUCT Bag = #{\n    items: Array<int>,\n}\nVAR a = 0\nVAR other = 0\n~ {\n    a = Bag#{items: #[1, 2, 3]}\n    other = #[9, 9, 9]\n    remove_at(a.items, 99)\n}\n{a.items[0]}\n-> END\n";
    let mut story = compile(source);
    let _ = run_to_completion_or_fault(&mut story).expect_err("index 99 is out of bounds");
    let other = story.variable("other").expect("other is declared");
    assert_eq!(
        other,
        &brink_format::Value::array(vec![
            brink_format::Value::Int(9),
            brink_format::Value::Int(9),
            brink_format::Value::Int(9),
        ])
    );
}
