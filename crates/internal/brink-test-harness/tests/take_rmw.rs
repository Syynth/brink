//! `TakeGlobal`/`TakeTemp` RMW-discipline tests (issue #576,
//! `docs/value-model-spec.md` §5).
//!
//! `crates/internal/brink-test-harness/tests/proptest_t1b.rs` already proves
//! RMW-equivalence and the sharing-unobservable law for the *chained*
//! (`grid[y][x] = v`) and mutator (`push`/`insert`/`remove`, already
//! bare-variable) cases; those keep passing unchanged (proving #576 didn't
//! alter observable behavior for the fallback path). This file adds the
//! coverage specific to #576's new code:
//!
//! - RMW-equivalence and the sharing law for the **flat** (`n == 1`,
//!   `a[i] = v`/`a[i] op= v` on a bare variable) fast path
//!   `lower_flat_indexed_assignment` added — the exact shape the
//!   loop-append benchmark exercises.
//! - The **fault-during-RMW slot state** property the issue requires: a
//!   mid-RMW fault must not silently corrupt unrelated state, and must be
//!   a defined, tested outcome for the variable being written.
//! - Reachability of `TakeTemp`'s pointer-auto-dereference branch (a `ref`
//!   parameter's indexed assignment inside a `~ { … }` block).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Line, RuntimeError, Story};
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
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Suspended { text, .. } => {
                out.push_str(&text);
                return Ok(out);
            }
            Line::Choices { .. } => return Ok(out),
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

// ── Flat (n == 1) indexed-assignment RMW-equivalence + sharing ──────────
//
// `proptest_t1b.rs` covers the chained (`grid[y][x] = v`) shape; these
// extend the same laws to the bare-variable fast path #576 adds.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// `a[i] = v` on a compiled brink program matches the manual
    /// take/mutate/write-back a Rust reference performs on an equivalent
    /// `Vec<i32>` — RMW-equivalence for the flat fast path.
    #[test]
    fn flat_index_assignment_matches_manual_rmw(
        base in prop::collection::vec(-1000i32..1000, 1..8),
        i in 0usize..8,
        v in -1000i32..1000,
    ) {
        prop_assume!(i < base.len());
        let mut reference = base.clone();
        reference[i] = v;

        let literal = format!(
            "#[{}]",
            base.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
        );
        let source = format!(
            "VAR arr = 0\nVAR out = \"\"\n~ {{\n    arr = {literal}\n    arr[{i}] = {v}\n    for x in arr {{\n        out = out + \" \" + x\n    }}\n}}\n{{out}}\n-> END\n",
        );
        let mut story = compile(&source);
        let out = run_to_completion_or_fault(&mut story).expect("no fault expected");
        prop_assert_eq!(out.trim(), space_joined(&reference));
    }

    /// `a[i] += v` / `a[i] -= v` (compound flat indexed assignment) matches
    /// the manual reference.
    #[test]
    fn flat_index_compound_assignment_matches_manual_rmw(
        base in prop::collection::vec(-1000i32..1000, 1..8),
        i in 0usize..8,
        v in -1000i32..1000,
        subtract in any::<bool>(),
    ) {
        prop_assume!(i < base.len());
        let mut reference = base.clone();
        if subtract {
            reference[i] -= v;
        } else {
            reference[i] += v;
        }

        let op = if subtract { "-=" } else { "+=" };
        let literal = format!(
            "#[{}]",
            base.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
        );
        let source = format!(
            "VAR arr = 0\nVAR out = \"\"\n~ {{\n    arr = {literal}\n    arr[{i}] {op} {v}\n    for x in arr {{\n        out = out + \" \" + x\n    }}\n}}\n{{out}}\n-> END\n",
        );
        let mut story = compile(&source);
        let out = run_to_completion_or_fault(&mut story).expect("no fault expected");
        prop_assert_eq!(out.trim(), space_joined(&reference));
    }

    /// Sharing-unobservable law (value-model-spec §3) applied to the flat
    /// fast path: `b = a` then `b[i] = v` never changes `a`, whether or not
    /// `a`'s Arc was uniquely owned going in — the take-based RMW must COW
    /// exactly when something else is still watching.
    #[test]
    fn copy_then_flat_mutate_never_observes_through_the_original(
        base in prop::collection::vec(-1000i32..1000, 1..8),
        i in 0usize..8,
        v in -1000i32..1000,
    ) {
        prop_assume!(i < base.len());
        let original = base.clone();
        let mut mutated = base.clone();
        mutated[i] = v;

        let literal = format!(
            "#[{}]",
            base.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
        );
        let source = format!(
            "VAR a = 0\nVAR b = 0\nVAR out_a = \"\"\nVAR out_b = \"\"\n~ {{\n    a = {literal}\n    b = a\n    b[{i}] = {v}\n    for x in a {{\n        out_a = out_a + \" \" + x\n    }}\n    for x in b {{\n        out_b = out_b + \" \" + x\n    }}\n}}\n{{out_a}}\nSPLIT\n{{out_b}}\n-> END\n",
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

// ── Fault-during-RMW slot state (issue #576's required property) ────────

/// `a[i] = v` with `i` out of bounds faults. Renamed from
/// `..._leaves_root_unchanged` by issue #856: plain `=` no longer runs a
/// non-mutating pre-check before taking the root (that precheck existed
/// only to catch this same fault early — see `lower_flat_indexed_assignment`
/// doc — but it also faulted on a merely-absent map key, which #856 ruled
/// should insert instead; removing it for maps means removing it for
/// arrays too, since neither the compiler nor the runtime can tell the two
/// apart before the container is taken). So the root now ends up
/// `Value::Null` on this fault, matching the trade-off
/// `fault_during_insert_leaves_root_null`/`fault_during_remove_at_leaves_root_null`
/// below already document for `insert`/`remove_at`.
#[test]
fn fault_during_flat_index_assignment_leaves_root_null() {
    let source = "VAR arr = 0\n~ {\n    arr = #[1, 2, 3]\n    arr[10] = 99\n}\n{arr[0]}\n-> END\n";
    let mut story = compile(source);
    let err = run_to_completion_or_fault(&mut story).expect_err("index 10 is out of bounds");
    assert!(
        matches!(err, RuntimeError::IndexOutOfBounds { index: 10, len: 3 }),
        "unexpected error: {err:?}"
    );
    let arr = story.variable("arr").expect("arr is declared");
    assert_eq!(
        arr,
        &brink_format::Value::Null,
        "documented fault-during-RMW slot state (issue #856): the taken root \
         is Value::Null, never a corrupted/partial container"
    );
}

/// Compound flat indexed assignment (`a[i] += v`) is unaffected by #856 —
/// it still runs the pre-mutation `current` read (needed as the operand),
/// which still catches an out-of-bounds index before the root is taken, so
/// the root stays completely unchanged on this fault.
#[test]
fn fault_during_flat_index_compound_assignment_leaves_root_unchanged() {
    let source = "VAR arr = 0\n~ {\n    arr = #[1, 2, 3]\n    arr[10] += 99\n}\n{arr[0]}\n-> END\n";
    let mut story = compile(source);
    let err = run_to_completion_or_fault(&mut story).expect_err("index 10 is out of bounds");
    assert!(
        matches!(err, RuntimeError::IndexOutOfBounds { index: 10, len: 3 }),
        "unexpected error: {err:?}"
    );
    let arr = story.variable("arr").expect("arr is declared");
    assert_eq!(
        arr,
        &brink_format::Value::array(vec![
            brink_format::Value::Int(1),
            brink_format::Value::Int(2),
            brink_format::Value::Int(3),
        ]),
        "a fault during compound indexed assignment must leave the root completely unchanged"
    );
}

/// `push(a, v)` on a non-collection `a` faults (`NotIndexable`) — and, per
/// `lower_bare_mutator`'s `push`-specific pre-check, leaves `a`
/// **completely unchanged** (the `len()` pre-check that doubles as push's
/// key catches this before anything is taken).
#[test]
fn fault_during_push_leaves_root_unchanged() {
    let source = "VAR arr = 0\n~ {\n    arr = 5\n    push(arr, 99)\n}\n{arr}\n-> END\n";
    let mut story = compile(source);
    let err = run_to_completion_or_fault(&mut story).expect_err("pushing onto an int faults");
    assert!(
        matches!(err, RuntimeError::NotIndexable("int")),
        "unexpected error: {err:?}"
    );
    let arr = story.variable("arr").expect("arr is declared");
    assert_eq!(
        arr,
        &brink_format::Value::Int(5),
        "a fault during push must leave the root completely unchanged"
    );
}

/// `insert(a, k, v)` with `k` out of bounds faults — `insert`/`remove`
/// don't get push's free pre-check (an arbitrary author-supplied key can't
/// be validated without either a dedicated non-mutating "is this key
/// valid" primitive this issue doesn't add, or paying for the COW this
/// path exists to avoid), so this is the **documented, deliberate**
/// trade-off: the taken root ends up `Value::Null`, not corrupted, not
/// panicking, not UB — a defined, tested outcome consistent with this VM's
/// pre-existing no-rollback-on-fault model (a fault anywhere mid-turn
/// already leaves earlier same-turn mutations applied).
#[test]
fn fault_during_insert_leaves_root_null() {
    let source =
        "VAR arr = 0\n~ {\n    arr = #[1, 2, 3]\n    insert(arr, 99, 5)\n}\n{arr[0]}\n-> END\n";
    let mut story = compile(source);
    let err = run_to_completion_or_fault(&mut story).expect_err("index 99 is out of bounds");
    assert!(
        matches!(err, RuntimeError::IndexOutOfBounds { index: 99, len: 3 }),
        "unexpected error: {err:?}"
    );
    let arr = story.variable("arr").expect("arr is declared");
    assert_eq!(
        arr,
        &brink_format::Value::Null,
        "documented fault-during-RMW slot state for insert/remove: the taken \
         root is Value::Null, never a corrupted/partial container"
    );
}

/// `remove_at(a, i)` with `i` out of bounds — same documented `Value::Null`
/// outcome as `insert`'s fault case above (issue #1484: this used to be
/// `remove(a, i)`; the array-index leg is `remove_at` now).
#[test]
fn fault_during_remove_at_leaves_root_null() {
    let source =
        "VAR arr = 0\n~ {\n    arr = #[1, 2, 3]\n    remove_at(arr, 99)\n}\n{arr[0]}\n-> END\n";
    let mut story = compile(source);
    let err = run_to_completion_or_fault(&mut story).expect_err("index 99 is out of bounds");
    assert!(
        matches!(err, RuntimeError::IndexOutOfBounds { index: 99, len: 3 }),
        "unexpected error: {err:?}"
    );
    let arr = story.variable("arr").expect("arr is declared");
    assert_eq!(arr, &brink_format::Value::Null);
}

/// `remove(a, k)` on an *array* — the whole point of issue #1484's split:
/// `remove` is map-only now, so a container-kind mismatch (not an
/// out-of-bounds index) is the fault, `NotIndexable("array")` rather than
/// `IndexOutOfBounds`. Same documented `Value::Null` root-after-fault
/// outcome as `insert`/`remove_at` above — the container is taken before
/// the kind check runs.
#[test]
fn fault_during_remove_on_an_array_leaves_root_null() {
    let source =
        "VAR arr = 0\n~ {\n    arr = #[1, 2, 3]\n    remove(arr, 0)\n}\n{arr[0]}\n-> END\n";
    let mut story = compile(source);
    let err = run_to_completion_or_fault(&mut story).expect_err("remove on an array faults");
    assert!(
        matches!(err, RuntimeError::NotIndexable("array")),
        "unexpected error: {err:?}"
    );
    let arr = story.variable("arr").expect("arr is declared");
    assert_eq!(arr, &brink_format::Value::Null);
}

/// A fault during one variable's RMW must not corrupt an *unrelated*
/// global — `TakeGlobal`/`TakeTemp` only ever touch the one slot they're
/// given.
#[test]
fn fault_during_rmw_does_not_touch_unrelated_globals() {
    let source = "VAR arr = 0\nVAR other = 0\n~ {\n    arr = #[1, 2, 3]\n    other = #[9, 9, 9]\n    arr[10] = 99\n}\n{arr[0]}\n-> END\n";
    let mut story = compile(source);
    let _ = run_to_completion_or_fault(&mut story).expect_err("index 10 is out of bounds");
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

// ── TakeTemp pointer-auto-dereference reachability ───────────────────────

/// `TakeTemp`'s pointer-following branch (mirrors `GetTemp`/`SetTemp`'s
/// `ref`-param write-through) is reachable: a `ref` parameter's indexed
/// assignment inside a `~ { … }` block resolves to a temp slot holding a
/// `VariablePointer`, so `lower_flat_indexed_assignment`'s
/// `take_expr_for_target` compiles to `TakeTemp`, and the VM must take from
/// the *pointed-to* global, not the pointer value itself.
#[test]
fn ref_param_flat_indexed_assignment_takes_through_the_pointer() {
    let source = "VAR grid = 0\n~ {\n    grid = #[1, 2, 3]\n    bump(grid)\n}\n{grid[0]}\n-> END\n\n=== function bump(ref arr: Array<int>) ===\n~ {\n    arr[0] = arr[0] + 1\n}\n~ return 0\n";
    let mut story = compile(source);
    let out = run_to_completion_or_fault(&mut story).expect("no fault expected");
    assert_eq!(
        out.trim(),
        "2",
        "ref-param indexed assignment must write through to the caller's global"
    );
    let grid = story.variable("grid").expect("grid is declared");
    assert_eq!(
        grid,
        &brink_format::Value::array(vec![
            brink_format::Value::Int(2),
            brink_format::Value::Int(2),
            brink_format::Value::Int(3),
        ])
    );
}
