//! Shared compile/run helpers for the issue #672 workstream B ("laws") test
//! suites in this crate — `law_cow_sharing_programs.rs`,
//! `law_rmw_equivalence.rs`, `law_strict_gradual_equivalence.rs`. A
//! `tests/law_support/mod.rs` submodule (not a top-level `tests/*.rs` file)
//! so cargo doesn't treat it as its own test binary; each law file pulls it
//! in with `mod law_support;`.
//!
//! The compile/run shape mirrors `tests/proptest_t1b.rs` and
//! `tests/take_rmw.rs` (issue #570/#576) — this module exists so the three
//! new law files don't each re-derive it a third/fourth/fifth time.

#![allow(dead_code)]

use std::sync::Arc;

use brink_compiler::{AnalysisOptions, Dialect, TypePolicy};
use brink_runtime::{DotNetRng, Line, RuntimeError, Story};

/// Compile `source` under the brink dialect with the given type policy and
/// return a linked, unstarted [`Story`], or the compiler's `Err` untouched
/// (callers that need to assert a program is error-free under both policies
/// — value-model-spec's typed-mode companion, `docs/typed-mode-spec.md` §1
/// — want the `Result`, not a panic).
pub fn try_compile(source: &str, types: TypePolicy) -> Result<Story<DotNetRng>, String> {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        types,
        ..AnalysisOptions::default()
    };
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(source.to_string()), options)
            .map_err(|e| format!("{e:?}"))?;
    let (program, line_tables) = brink_runtime::link(&output.data).map_err(|e| format!("{e:?}"))?;
    Ok(Story::<DotNetRng>::new(Arc::new(program), line_tables))
}

/// [`try_compile`] under [`TypePolicy::Gradual`] (today's default), panicking
/// (test code, exempt via `clippy.toml`) with the source attached on
/// failure — the shape `proptest_t1b.rs`/`take_rmw.rs` already use for the
/// common case where a generated program is known-valid.
pub fn compile(source: &str) -> Story<DotNetRng> {
    try_compile(source, TypePolicy::Gradual)
        .unwrap_or_else(|e| panic!("compile error under Gradual for:\n{source}\n{e}"))
}

/// Run `story` to completion (choice-free), returning the concatenated text
/// on success or the `RuntimeError` that terminated the turn. Either way,
/// `story` is left exactly where execution stopped.
pub fn run_to_completion_or_fault(story: &mut Story<DotNetRng>) -> Result<String, RuntimeError> {
    let mut out = String::new();
    loop {
        match story.continue_single()? {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } | Line::Choices { text, .. } => {
                out.push_str(&text);
                return Ok(out);
            }
        }
    }
}

/// [`run_to_completion_or_fault`], panicking (test code) on a runtime fault
/// — the common case for a generated program known not to fault.
pub fn run_to_completion(story: &mut Story<DotNetRng>) -> String {
    run_to_completion_or_fault(story).unwrap_or_else(|e| panic!("runtime error: {e:?}"))
}

/// Space-separated `i32`s — the accumulation shape `proptest_t1b.rs` reads
/// mutated collections back through ink's text-output surface with
/// (`out = out + " " + x`, `.trim()`'d at the comparison site).
pub fn space_joined(values: &[i32]) -> String {
    values
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}
