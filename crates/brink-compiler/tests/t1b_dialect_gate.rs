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

// ── brink dialect still rejects — nothing lowers to LIR in T1b-1 ─────

#[test]
fn brink_dialect_rejects_block_as_not_yet_implemented() {
    let source = "~ {\ntemp x = 0\n}\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E052
            && d.message.contains("not yet implemented")
            && d.message.contains("T1b-2")),
        "{diags:?}"
    );
}

#[test]
fn brink_dialect_rejects_array_literal_as_not_yet_implemented() {
    let source = "~ x = #[1, 2, 3]\n";
    let err = compile_mem_with_dialect(source, Dialect::Brink).unwrap_err();
    let diags = diagnostics_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E052),
        "{diags:?}"
    );
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
