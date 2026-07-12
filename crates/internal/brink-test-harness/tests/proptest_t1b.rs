//! T1b property tests (`docs/t1b-surface-spec.md` §6, issue #570).
//!
//! Generates small random brink-dialect programs and checks them against an
//! independent Rust reference computation — proving the compiled bytecode's
//! *behavior*, not just that two code paths agree with each other. Sizes are
//! kept small (bounded array dimensions, bounded map entry counts) so the
//! generated `.ink` source stays small and each case compiles+runs fast; the
//! VM's own step limit is the backstop against runaway generated loops
//! (CLAUDE.md "guard against unbounded growth").

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Grid dimensions are bounded to 2..=4 (`arb_grid_dims`), so `usize -> i32`
// cell-value casts below never truncate/wrap in practice.
#![allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Line, Story};
use proptest::prelude::*;

/// Compile+run `source` under the brink dialect to completion (choice-free).
/// Panics (test code, exempt via `clippy.toml`) with the source attached, so
/// a proptest shrink failure is immediately actionable.
fn run_brink(source: &str) -> String {
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
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let step_msg = format!("runtime error for:\n{source}");
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
    assert!(!hit_choices, "unexpected choices for:\n{source}");
    out
}

fn arb_grid_dims() -> impl Strategy<Value = (usize, usize)> {
    (2usize..=4, 2usize..=4)
}

/// Build `#[#[..],#[..],...]` literal text for an `h x w` grid of
/// `row*w + col` values (a fixed, checkable content — the property under
/// test is the *mutation*, not the literal itself).
fn grid_literal(h: usize, w: usize) -> String {
    let rows: Vec<String> = (0..h)
        .map(|r| {
            let cells: Vec<String> = (0..w).map(|c| (r * w + c).to_string()).collect();
            format!("#[{}]", cells.join(", "))
        })
        .collect();
    format!("#[{}]", rows.join(", "))
}

/// A global declaration + `~` block + interpolation that flattens `var` (a
/// grid of grids) into a space-separated string via nested `for`-in loops —
/// reads a mutated grid back out through ink's own text-output surface,
/// proving the mutation via observable behavior rather than internal
/// inspection. Accumulates into a *global* (`{acc_var}`), not a block-scoped
/// `temp` — a block-scoped temp goes out of scope at the end of its `~ {{ }}`
/// block (`docs/t1b-surface-spec.md` §2), so it can't be read from the
/// interpolation that follows the block.
fn print_flatten_expr(var: &str, acc_var: &str) -> String {
    format!(
        "VAR {acc_var} = \"\"\n~ {{\n    for row in {var} {{\n        for cell in row {{\n            {acc_var} = {acc_var} + \" \" + cell\n        }}\n    }}\n}}\n{{{acc_var}}}"
    )
}

/// Reference row-major flattening matching `print_flatten_expr`'s output
/// shape (space-prefixed cells, ink `INT -> string` stringification).
fn flatten_grid(grid: &[Vec<i32>]) -> String {
    let mut out = String::new();
    for row in grid {
        for cell in row {
            out.push(' ');
            out.push_str(&cell.to_string());
        }
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// RMW chain equivalence: `grid[y][x] = v` on a compiled brink program
    /// must equal the manual take/mutate/write-back a Rust reference
    /// implementation performs on an equivalent `Vec<Vec<i32>>` — the exact
    /// law `docs/t1b-surface-spec.md` §4 requires ("chains lower to nested
    /// RMW... take -> make_mut -> write-back").
    #[test]
    fn grid_index_assignment_matches_manual_rmw(
        (h, w) in arb_grid_dims(),
        y in 0usize..4,
        x in 0usize..4,
        v in -1000i32..1000,
    ) {
        prop_assume!(y < h && x < w);

        let mut reference: Vec<Vec<i32>> = (0..h)
            .map(|r| (0..w).map(|c| (r * w + c) as i32).collect())
            .collect();
        reference[y][x] = v;

        let source = format!(
            "VAR grid = 0\n~ {{\n    grid = {}\n    grid[{y}][{x}] = {v}\n}}\n{}\n-> END\n",
            grid_literal(h, w),
            print_flatten_expr("grid", "acc"),
        );
        let out = run_brink(&source);
        let expected = flatten_grid(&reference);
        prop_assert_eq!(out.trim(), expected.trim());
    }

    /// Map iteration order is insertion order, deterministically — no
    /// dependence on key hashing or any other non-deterministic factor
    /// (value-model-spec §4's ruling; CLAUDE.md's HashMap-ordering religion
    /// applies equally to this ratified user-visible ordering guarantee).
    #[test]
    fn map_iterates_keys_in_insertion_order(
        keys in prop::collection::vec("[a-e]", 1..6),
    ) {
        // Reference: first-insertion-position semantics (matches
        // `OrderedMap::insert` — a repeated key keeps its original slot).
        let mut expected_order: Vec<String> = Vec::new();
        for k in &keys {
            if !expected_order.contains(k) {
                expected_order.push(k.clone());
            }
        }

        let entries: Vec<String> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| format!("\"{k}\": {i}"))
            .collect();
        let source = format!(
            "VAR out = \"\"\n~ {{\n    temp m = #{{{}}}\n    for k in m {{\n        out = out + k\n    }}\n}}\n{{out}}\n-> END\n",
            entries.join(", "),
        );
        let out = run_brink(&source);
        prop_assert_eq!(out.trim(), expected_order.join(""));
    }

    /// Sharing-unobservable law (value-model-spec §3): assigning `b = a`
    /// then mutating `b` through an indexed write never changes `a` — a
    /// randomized block program can never observe the underlying `Arc`
    /// sharing, only value semantics.
    #[test]
    fn copy_then_mutate_never_observes_through_the_original(
        (h, w) in arb_grid_dims(),
        y in 0usize..4,
        x in 0usize..4,
        v in -1000i32..1000,
    ) {
        prop_assume!(y < h && x < w);

        let original: Vec<Vec<i32>> = (0..h)
            .map(|r| (0..w).map(|c| (r * w + c) as i32).collect())
            .collect();
        let mut mutated = original.clone();
        mutated[y][x] = v;

        let source = format!(
            "VAR a = 0\nVAR b = 0\n~ {{\n    a = {}\n    b = a\n    b[{y}][{x}] = {v}\n}}\n{}\nSPLIT\n{}\n-> END\n",
            grid_literal(h, w),
            print_flatten_expr("a", "acc_a"),
            print_flatten_expr("b", "acc_b"),
        );
        let out = run_brink(&source);
        let mut parts = out.split("SPLIT");
        let a_text = parts.next().unwrap_or_default();
        let b_text = parts.next().unwrap_or_default();
        let expected_a = flatten_grid(&original);
        let expected_b = flatten_grid(&mutated);
        prop_assert_eq!(a_text.trim(), expected_a.trim());
        prop_assert_eq!(b_text.trim(), expected_b.trim());
    }
}
