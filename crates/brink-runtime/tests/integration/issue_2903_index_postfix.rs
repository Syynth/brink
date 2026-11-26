//! Issue #2903 — index-operand postfix (`a[0]++`, `m["k"]++`) is a silent
//! non-mutation on both the `~ { … }` block surface and the classic-line
//! surface, proven through the real pipeline (`brink_compiler::compile_with_options`
//! → link → `Story`), not just unit LIR lowering.
//!
//! Sibling of #2894 (bare-variable postfix inside a block, fixed by PR
//! #2900): PR #2900's review found the same silent-drop for an
//! **index-operand** postfix. `blocks::try_lower_postfix_stmt`'s field-operand
//! guard (`reject_field_projection_index_root`) only matches `Path`/
//! `FieldAccess` operands — an `Index` operand matches neither, so
//! `lower_assign_target`'s `_ => None` fallthrough drops the postfix into the
//! pure-expression fallback: the value is computed and discarded, with no
//! diagnostic at all. `stmts.rs`'s classic-line `ExprStmt` arm delegates to
//! the same shared helper (post-#2900 review fix), so both surfaces share one
//! fix point — these tests exercise both.
//!
//! `list_index_postfix_*` / `map_key_postfix_*` were RED before the fix (the
//! story printed the *unchanged* initial value instead of the
//! incremented/decremented one). `bare_variable_postfix_*` and
//! `field_operand_postfix_*` are regression tests for the two established
//! interplay guarantees (#2900's bare-variable fix, #2185/#2897's E074
//! field-operand refusal) that this fix must not disturb.
//!
//! Nested shapes (issue's "enumerate what each does today" ask):
//! - `a[0].count++` — the postfix operand is `FieldAccess { base: Index(..),
//!   field: count }`, an outer `FieldAccess` shape — already refused with
//!   E074 by the pre-existing `reject_field_projection_index_root` match arm,
//!   unaffected by this fix (`nested_field_of_index_postfix_refuses_with_e074`
//!   locks this in as a regression).
//! - `p.items[0]++` — the postfix operand is `Index { base: Path("p.items"),
//!   .. }`, an outer `Index` shape whose *flattened* root is the
//!   struct-field-projected `Path` `p.items` — this hit the exact same
//!   silent-drop bug as a bare-variable index target. Routing through
//!   `lower_indexed_assignment` (this fix) makes it reuse the identical
//!   `reject_field_projection_index_root` check #2121 already proved correct
//!   for `p.items[0] = v`, so it now refuses with E074 instead of silently
//!   dropping (`nested_index_of_field_projection_postfix_refuses_with_e074`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Step, Story};

fn brink_options() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

fn compile_brink(source: &str) -> brink_format::StoryData {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
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
        brink_options(),
    )
    .expect("brink-dialect source should compile clean")
    .data
}

fn compile_brink_expect_diagnostics(source: &str) -> Vec<brink_compiler::ResolvedDiagnostic> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let err = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        brink_options(),
    )
    .expect_err("expected a diagnostics compile error");
    let brink_compiler::CompileError::Diagnostics(diags) = err else {
        panic!("expected a Diagnostics compile error, got a different CompileError variant");
    };
    diags
}

fn run_to_completion(data: &brink_format::StoryData) -> String {
    let (program, line_tables) = brink_runtime::link(data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut output = String::new();
    loop {
        match story.continue_single().unwrap() {
            Step::Line(line) => output.push_str(&line.text),
            Step::Done | Step::End | Step::Suspended => return output,
            Step::Choices(_) => panic!("fixture has no choices"),
        }
    }
}

// ── List index postfix — block surface ───────────────────────────────────

#[test]
fn list_index_postfix_increment_inside_block_mutates_end_to_end() {
    let src = "VAR a = #[1, 2, 3]\n~ {\n    a[0]++\n}\n{a[0]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "2",
        "`~ {{ a[0]++ }}` must mutate a[0] from 1 to 2 — got {result:?} (issue #2903: \
         Index-operand postfix computed a discarded value and never wrote it back)"
    );
}

#[test]
fn list_index_postfix_decrement_inside_block_mutates_end_to_end() {
    let src = "VAR a = #[1, 2, 3]\n~ {\n    a[0]--\n}\n{a[0]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "0",
        "`~ {{ a[0]-- }}` must mutate a[0] from 1 to 0 — got {result:?}"
    );
}

// ── List index postfix — classic-line surface ────────────────────────────

#[test]
fn list_index_postfix_increment_classic_line_mutates_end_to_end() {
    let src = "VAR a = #[1, 2, 3]\n~ a[0]++\n{a[0]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "2",
        "`~ a[0]++` must mutate a[0] from 1 to 2 — got {result:?} (issue #2903)"
    );
}

#[test]
fn list_index_postfix_decrement_classic_line_mutates_end_to_end() {
    let src = "VAR a = #[1, 2, 3]\n~ a[0]--\n{a[0]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "0",
        "`~ a[0]--` must mutate a[0] from 1 to 0 — got {result:?}"
    );
}

// ── Map key postfix — block surface ──────────────────────────────────────

#[test]
fn map_key_postfix_increment_inside_block_mutates_end_to_end() {
    let src = "VAR m = #{\"k\": 1}\n~ {\n    m[\"k\"]++\n}\n{m[\"k\"]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "2",
        "`~ {{ m[\"k\"]++ }}` must mutate m[\"k\"] from 1 to 2 — got {result:?} (issue #2903)"
    );
}

#[test]
fn map_key_postfix_decrement_inside_block_mutates_end_to_end() {
    let src = "VAR m = #{\"k\": 1}\n~ {\n    m[\"k\"]--\n}\n{m[\"k\"]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "0",
        "`~ {{ m[\"k\"]-- }}` must mutate m[\"k\"] from 1 to 0 — got {result:?}"
    );
}

// ── Map key postfix — classic-line surface ───────────────────────────────

#[test]
fn map_key_postfix_increment_classic_line_mutates_end_to_end() {
    let src = "VAR m = #{\"k\": 1}\n~ m[\"k\"]++\n{m[\"k\"]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "2",
        "`~ m[\"k\"]++` must mutate m[\"k\"] from 1 to 2 — got {result:?} (issue #2903)"
    );
}

#[test]
fn map_key_postfix_decrement_classic_line_mutates_end_to_end() {
    let src = "VAR m = #{\"k\": 1}\n~ m[\"k\"]--\n{m[\"k\"]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "0",
        "`~ m[\"k\"]--` must mutate m[\"k\"] from 1 to 0 — got {result:?}"
    );
}

// ── Proof: the `+=`/`-=` RMW path (`lower_indexed_assignment`) this fix
// routes postfix through is already correct for BOTH a list index and a map
// key — required before preferring the routing fix over an outright E074
// refusal (issue #2903's "PREFER routing IFF the RMW path is proven correct
// for both list index and map key"). The list-index leg is already proven
// exhaustively by `take_rmw.rs`'s proptest suite; `law_rmw_equivalence.rs`
// proves `m[key] = v` but has no map-key `+=` case, so this closes that gap
// with a direct end-to-end run through the real pipeline. ─────────────────

#[test]
fn map_key_compound_add_assign_matches_manual_rmw_end_to_end() {
    let src = "VAR m = #{\"k\": 1}\n~ {\n    m[\"k\"] += 4\n}\n{m[\"k\"]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "5",
        "`m[\"k\"] += 4` must produce 1 + 4 = 5 — got {result:?} (proves the RMW path \
         postfix routes through is correct for a map key before this fix relies on it)"
    );
}

#[test]
fn map_key_compound_sub_assign_matches_manual_rmw_end_to_end() {
    let src = "VAR m = #{\"k\": 5}\n~ {\n    m[\"k\"] -= 4\n}\n{m[\"k\"]}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "1",
        "`m[\"k\"] -= 4` must produce 5 - 4 = 1 — got {result:?}"
    );
}

// ── Regression: bare-variable postfix must keep mutating (PR #2900) ─────

#[test]
fn bare_variable_postfix_increment_classic_line_still_mutates_end_to_end() {
    let src = "VAR x = 5\n~ x++\n{x}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "6",
        "regression: `~ x++` classic-line must still mutate x — got {result:?}"
    );
}

#[test]
fn bare_variable_postfix_increment_inside_block_still_mutates_end_to_end() {
    let src = "VAR x = 5\n~ {\n    x++\n}\n{x}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "6",
        "regression: `~ {{ x++ }}` must still mutate x — got {result:?}"
    );
}

// ── Regression: field-operand postfix must keep refusing E074 (#2185/#2897) ─

#[test]
fn field_operand_postfix_classic_line_still_refuses_with_e074() {
    let src = "STRUCT Bag = #{count: int, tag: string}\n\
        VAR a = Bag#{count: 5, tag: \"hello\"}\n~ a.count++\nHello.\n-> END\n";
    let diags = compile_brink_expect_diagnostics(src);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E074),
        "regression: expected E074 for classic-line `a.count++`, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn field_operand_postfix_inside_block_still_refuses_with_e074() {
    let src = "STRUCT Bag = #{count: int, tag: string}\n\
        VAR a = Bag#{count: 5, tag: \"hello\"}\n~ {\n    a.count++\n}\nHello.\n-> END\n";
    let diags = compile_brink_expect_diagnostics(src);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E074),
        "regression: expected E074 for block `a.count++`, got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

// ── Nested shapes (issue's "enumerate what each does today" ask) ────────

/// `a[0].count++` — outer shape is `FieldAccess { base: Index(a[0]), field:
/// count }`. Already refused with E074 today via the pre-existing
/// `reject_field_projection_index_root` `FieldAccess` match arm — this fix
/// does not touch that arm, so this must keep refusing, not start silently
/// mutating or misrouting.
#[test]
fn nested_field_of_index_postfix_refuses_with_e074() {
    let src = "STRUCT Bag = #{count: int, tag: string}\n\
        VAR a = #[Bag#{count: 5, tag: \"x\"}]\n~ {\n    a[0].count++\n}\nHello.\n-> END\n";
    let diags = compile_brink_expect_diagnostics(src);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E074),
        "expected E074 for `a[0].count++` (chained field write projection), got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}

/// `p.items[0]++` — outer shape is `Index { base: Path("p.items"), .. }`,
/// whose *flattened* root (after unwinding the index chain) is the
/// struct-field-projected multi-segment `Path` `p.items`. Before this fix
/// this silently dropped the whole statement (same #2903 root cause as a
/// bare-variable index target, just with a field-projected root). After this
/// fix it must refuse with the same E074 issue #2121 already established for
/// `p.items[0] = v` — not silently drop, and not misroute the write onto the
/// whole `p` record.
#[test]
fn nested_index_of_field_projection_postfix_refuses_with_e074() {
    let src = "STRUCT Bag = #{items: Array<int>, tag: string}\n\
        VAR p = Bag#{items: #[1, 2, 3], tag: \"x\"}\n~ {\n    p.items[0]++\n}\nHello.\n-> END\n";
    let diags = compile_brink_expect_diagnostics(src);
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E074),
        "expected E074 for `p.items[0]++` (field-projected index root), got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}
