//! End-to-end T1b-1 dialect gate tests (docs/t1b-surface-spec.md §1, #569).
//!
//! Exercises the full pipeline the way a real caller would: parse → HIR
//! lower → analyze → (never reaches) LIR lower → codegen, through the public
//! `brink_compiler::compile_with_options` entry point. Proves the concrete
//! consumer path: `AnalysisOptions::dialect` flows from the CLI/library
//! caller through `brink-driver` → `brink-db`'s `lir_query` diagnostic gate
//! → `CompileError::Diagnostics`.

#![allow(clippy::panic)]

use std::collections::HashMap;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_ir::DiagnosticCode;

fn compile_mem_with_dialect(
    source: &str,
    dialect: Dialect,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    let options = AnalysisOptions {
        dialect,
        ..AnalysisOptions::default()
    };
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
        options,
    )
}

fn diagnostics_of(err: brink_compiler::CompileError) -> Vec<brink_compiler::ResolvedDiagnostic> {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => diags,
        other => panic!("expected Diagnostics error, got {other:?}"),
    }
}

// ── Default dialect is strict-ink ────────────────────────────────────

#[test]
fn default_options_dialect_is_strict_ink() {
    assert_eq!(AnalysisOptions::default().dialect, Dialect::StrictInk);
}

// ── strict-ink rejects every extension construct ─────────────────────

#[test]
fn strict_ink_rejects_multiline_block() {
    let source = "~ {\ntemp x = 0\nx = x + 1\n}\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E051
            && d.message.contains("brink extension")
            && d.message.contains("multi-line logic block")),
        "{diags:?}"
    );
}

#[test]
fn strict_ink_rejects_array_literal() {
    let source = "~ x = #[1, 2, 3]\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E051 && d.message.contains("array literal")),
        "{diags:?}"
    );
}

#[test]
fn strict_ink_rejects_map_literal() {
    let source = "~ x = #{}\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E051 && d.message.contains("map literal")),
        "{diags:?}"
    );
}

#[test]
fn strict_ink_rejects_indexing() {
    let source = "~ x = a[0]\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags
            .iter()
            .any(|d| d.code == DiagnosticCode::E051 && d.message.contains("indexing")),
        "{diags:?}"
    );
}

#[test]
fn strict_ink_rejects_indexed_assignment() {
    let source = "~ a[0] = 5\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E051),
        "{diags:?}"
    );
}

// ── brink dialect compiles every T1b construct since T1b-2 (#570) ────

#[test]
fn brink_dialect_compiles_a_logic_block() {
    let source = "~ {\ntemp x = 0\nx = x + 1\n}\nHello, world!\n";
    let out = compile_mem_with_dialect(source, Dialect::Brink).unwrap();
    assert!(!out.data.containers.is_empty());
}

#[test]
fn brink_dialect_compiles_an_array_literal() {
    let source = "VAR x = 0\n~ x = #[1, 2, 3]\nHello, world!\n";
    let out = compile_mem_with_dialect(source, Dialect::Brink).unwrap();
    assert!(!out.data.containers.is_empty());
}

// ── Plain ink is unaffected by either dialect ─────────────────────────

#[test]
fn plain_ink_compiles_under_strict_ink() {
    let source = "VAR x = 5\n~ x = x + 1\nHello, world!\n";
    let out = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap();
    assert!(!out.data.containers.is_empty());
}

#[test]
fn plain_ink_compiles_under_brink_dialect() {
    let source = "VAR x = 5\n~ x = x + 1\nHello, world!\n";
    let out = compile_mem_with_dialect(source, Dialect::Brink).unwrap();
    assert!(!out.data.containers.is_empty());
}

/// `if`/`while`/`for`/`break`/`continue`/`in` are contextual keywords — a
/// story using one of those words as an ordinary variable name (outside a
/// `~ { … }` block) must still compile under both dialects, byte-identically
/// to how it always has.
#[test]
fn contextual_keyword_words_as_plain_identifiers_are_unaffected() {
    let source = "VAR for = 1\nVAR if = 0\n~ if = for + 1\nHello, world!\n";
    for dialect in [Dialect::StrictInk, Dialect::Brink] {
        let out = compile_mem_with_dialect(source, dialect).unwrap();
        assert!(!out.data.containers.is_empty());
    }
}

// ── T1b-2 (#570): a suppressed gate no longer risks silent corruption ─
//
// E051/E052 are *analysis* diagnostics, and analysis diagnostics are
// suppressible via `// brink-disable-all` (and per-line directives). Through
// T1b-1, suppressing the gate let extension syntax silently reach codegen as
// dropped data (`~ { … }` vanishing) or corrupted data (`a[0]` silently
// becoming `null`) — caught by the non-suppressible E053 LIR-lowering
// backstop, which refused to compile at all (#572 review). T1b-2 replaces
// that backstop with real lowering: a suppressed gate under `strict-ink` now
// just compiles the construct correctly (the same risk profile as
// suppressing any other diagnostic in this codebase), proven here by
// checking the array literal actually holds `[1, 2, 3]` at runtime — not
// silently dropped, not silently null.

#[test]
fn disable_all_lets_a_logic_block_compile_correctly_under_strict_ink() {
    let source = "// brink-disable-all\nHello\n~ {\ntemp x = 0\nx = x + 1\n}\nWorld\n";
    let out = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap();
    assert!(!out.data.containers.is_empty());
}

#[test]
fn disable_all_lets_indexing_compile_and_run_correctly_under_strict_ink() {
    let source =
        "// brink-disable-all\nVAR a = 0\nVAR x = 0\n~ a = #[10, 20, 30]\n~ x = a[1]\nvalue: {x}\n";
    let out = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap();
    let (program, line_tables) = brink_runtime::link(&out.data).unwrap();
    let mut story = brink_runtime::Story::<brink_runtime::DotNetRng>::new(
        std::sync::Arc::new(program),
        line_tables,
    );
    let line = story.continue_single().unwrap();
    let text = match line {
        brink_runtime::Step::Line(line) => line.text,
        other => panic!("expected text output, got {other:?}"),
    };
    assert!(
        text.contains("20"),
        "indexing must read the real value, not silently become null: {text:?}"
    );
}

// ── T1c-1 (#699): strict-ink rejects `#fn(…)` at analysis ───────────────

#[test]
fn strict_ink_rejects_fn_literal() {
    let source = "=== function heal(hp) ===\n~ return hp + 1\n\n\
                  === main ===\n~ temp f = #fn(heal, 1)\nDone.\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E051
            && d.message.contains("brink extension")
            && d.message.contains("#fn")),
        "{diags:?}"
    );
}

// ── T1e-1 (#831): strict-ink rejects `ref lvalue-path` at analysis ──────

#[test]
fn strict_ink_rejects_ref_expr() {
    // The oracle/strict-ink corpus is entirely classic ink — it never
    // spells `ref` at an argument's expression position (only the
    // pre-existing declaration-site `ref` param form, e.g.
    // `=== function heal(ref hp) ===`, which this grammar addition doesn't
    // touch at all). This proves the corpus stays untouched: any use of the
    // new T1e call-site form is a hard E051, same as every other T1b/T1c
    // brink extension.
    let source = "VAR gold = 5\n\
                  === function alter(ref x, k) ===\n~ x = x + k\n\n\
                  === main ===\n~ alter(ref gold, 1)\nDone.\n-> END\n";
    let err = compile_mem_with_dialect(source, Dialect::StrictInk).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E051
            && d.message.contains("brink extension")
            && d.message.contains("ref")),
        "{diags:?}"
    );
}
