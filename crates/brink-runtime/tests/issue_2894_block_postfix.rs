//! Issue #2894 — bare-variable postfix `x++`/`x--` inside a `~ { … }` block
//! must actually mutate the variable, proven through the real pipeline
//! (`brink_compiler::compile_with_options` → link → `Story`), not just unit
//! LIR lowering.
//!
//! `blocks.rs`'s `BlockStmt::ExprStmt` arm had no postfix-to-`Assign`
//! conversion the way `stmts.rs`'s classic-line arm does (issue #2185/PR
//! #2897's fix), so `lower_expr` lowered a bare-variable postfix inside a
//! block to a pure, discarded `lir::ExprKind::Postfix` — it computed `x + 1`/
//! `x - 1` and threw the result away, with no diagnostic. These tests
//! compile a real `.ink` source using the `brink` dialect extension (the
//! only surface `~ { … }` parses on), run it to completion, and assert the
//! variable's *printed* value actually changed — proving the write reaches
//! the runtime, not just that lowering produces some `Stmt`.
//!
//! `bare_variable_postfix_field_operand_still_refuses_with_e074` is the
//! CRITICAL interplay check the issue calls out: the fix must route a
//! field-operand postfix (`a.count++`) inside a block to the SAME
//! non-suppressible `E074` refusal issue #2185/PR #2897 established for the
//! classic-line spelling, rather than reintroducing that misroute (a write
//! landing on the whole record root instead of the field) for the block
//! surface.

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
    .expect("brink-dialect block source should compile clean")
    .data
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

/// RED (pre-fix): a `~ { x++ }` block silently never mutated `x` — the
/// story printed the unchanged initial value "5" instead of "6". This is
/// the issue's mandated real-pipeline proof, not a unit-lowering check.
#[test]
fn bare_variable_postfix_increment_inside_block_mutates_end_to_end() {
    let src = "VAR x = 5\n~ {\n    x++\n}\n{x}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "6",
        "`~ {{ x++ }}` must mutate `x` from 5 to 6 — got {result:?} (the #2894 \
         non-mutation bug: the postfix computed a discarded value and never wrote it back)"
    );
}

/// The `x--` sibling of the increment test above.
#[test]
fn bare_variable_postfix_decrement_inside_block_mutates_end_to_end() {
    let src = "VAR x = 5\n~ {\n    x--\n}\n{x}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "4",
        "`~ {{ x-- }}` must mutate `x` from 5 to 4 — got {result:?} (the #2894 \
         non-mutation bug)"
    );
}

/// A postfix inside a `while` loop's body (still `BlockStmt::ExprStmt`,
/// just nested one level deeper) — the fix must apply uniformly to every
/// `~ { … }` statement position, not just a block's top level.
#[test]
fn bare_variable_postfix_increment_inside_while_loop_body_mutates_end_to_end() {
    let src = "VAR x = 0\n~ {\n    while x < 3 {\n        x++\n    }\n}\n{x}\n-> END\n";
    let data = compile_brink(src);
    let result = run_to_completion(&data);
    assert_eq!(
        result.trim(),
        "3",
        "`x++` inside a `while` block body must mutate `x` — got {result:?}"
    );
}

/// CRITICAL interplay (#2185/PR #2897 + #2892): a field-operand postfix
/// (`a.count++`) inside a block must still refuse with the same
/// non-suppressible E074 the classic-line arm raises — not silently
/// misroute the write onto the whole record root the way #2185 did before
/// its fix. Proves the fix's field-projection guard actually runs on the
/// block surface, through the real compile pipeline.
#[test]
fn bare_variable_postfix_field_operand_still_refuses_with_e074() {
    let src = "STRUCT Bag = #{count: int, tag: string}\n\
        VAR a = Bag#{count: 5, tag: \"hello\"}\n~ {\n    a.count++\n}\nHello.\n-> END\n";
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", src)]);
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
    .expect_err("`a.count++` inside a block must be refused, not silently misrouted");

    let brink_compiler::CompileError::Diagnostics(diags) = err else {
        panic!("expected a Diagnostics compile error, got a different CompileError variant");
    };
    assert!(
        diags
            .iter()
            .any(|d| d.code == brink_compiler::DiagnosticCode::E074),
        "expected E074 (field-projection postfix target) for `a.count++` inside a block, \
         got: {:?}",
        diags.iter().map(|d| d.code.as_str()).collect::<Vec<_>>()
    );
}
