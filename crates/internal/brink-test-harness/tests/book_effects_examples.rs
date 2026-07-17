//! Compile-checks the "Effects" book chapter's fenced examples (T2-4, issue
//! #863), the same discipline the "Types"/"Function Values" chapters follow
//! (decision-log 2026-07-10: "Book code examples must be compile-checked").
//!
//! `mdbook test` only runs Rust doctests; `ink` fences are prose illustration
//! to it. This is the ink-side counterpart for the Effects chapter: it parses
//! `docs/book/src/toolchain/dialect/effects.md`'s fenced blocks straight out of
//! the shipped file and checks each under `Dialect::Brink`:
//!
//! - a ```` ```ink ```` block must compile + run to completion, and — where a
//!   ```` ```text ```` block immediately follows (the chapter's convention) —
//!   its output must match byte-for-byte;
//! - a ```` ```ink,error ```` block illustrates a compile *error* and must fail
//!   to compile with `E103` (the exceedance diagnostic the chapter is showing).
//!
//! An edit to the chapter that breaks either kind of example fails this test.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use brink_compiler::{AnalysisOptions, DiagnosticCode, Dialect};
use brink_runtime::{DotNetRng, Line, Story};

fn chapter_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("docs")
        .join("book")
        .join("src")
        .join("toolchain")
        .join("dialect")
        .join("effects.md")
}

/// One fenced code block: its info string (`ink`, `ink,error`, `text`, …) and
/// body text (the lines between the fences).
struct Fence {
    lang: String,
    body: String,
}

/// Extract every fenced code block, in document order. Simple by design (no
/// nested fences in this chapter): a line starting with ```` ``` ```` toggles
/// fence state; the info string is whatever follows it.
fn extract_fences(markdown: &str) -> Vec<Fence> {
    let mut fences = Vec::new();
    let mut lines = markdown.lines();
    while let Some(line) = lines.next() {
        let Some(lang) = line.strip_prefix("```") else {
            continue;
        };
        let mut body = String::new();
        for inner in lines.by_ref() {
            if inner == "```" {
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        fences.push(Fence {
            lang: lang.trim().to_owned(),
            body,
        });
    }
    fences
}

fn brink_opts() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

/// Compile `ink_src` (brink dialect) and run to completion, returning the
/// concatenated output. Panics on any compile/runtime error or a choice —
/// every runnable example in this chapter is a choice-free straight-line
/// program.
fn run_ink(ink_src: &str) -> String {
    let output =
        brink_compiler::compile_with_options("main.ink", |_| Ok(ink_src.to_owned()), brink_opts())
            .unwrap_or_else(|e| {
                panic!("chapter example failed to compile: {e:?}\n--- source ---\n{ink_src}")
            });
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);
    let mut out = String::new();
    loop {
        match story
            .continue_single()
            .unwrap_or_else(|e| panic!("chapter example faulted: {e:?}\n--- source ---\n{ink_src}"))
        {
            Line::Text { text, .. } => out.push_str(&text),
            Line::Done { text, .. } | Line::End { text, .. } => {
                out.push_str(&text);
                break;
            }
            Line::Choices { .. } => {
                panic!("chapter example presented choices (must be straight-line):\n{ink_src}")
            }
        }
    }
    out
}

/// The diagnostic codes a compile of `ink_src` produced (empty on success).
fn error_codes(ink_src: &str) -> Vec<DiagnosticCode> {
    match brink_compiler::compile_with_options("main.ink", |_| Ok(ink_src.to_owned()), brink_opts())
    {
        Ok(_) => Vec::new(),
        Err(brink_compiler::CompileError::Diagnostics(diags)) => {
            diags.iter().map(|d| d.code).collect()
        }
        Err(other) => panic!("unexpected compile error shape: {other:?}"),
    }
}

#[test]
fn every_ink_example_in_the_effects_chapter_checks_out() {
    let markdown = std::fs::read_to_string(chapter_path()).expect("read effects.md");
    let fences = extract_fences(&markdown);

    let mut ok_examples = 0;
    let mut error_examples = 0;
    let mut i = 0;
    while i < fences.len() {
        match fences[i].lang.as_str() {
            "ink" => {
                ok_examples += 1;
                let actual = run_ink(&fences[i].body);
                // Convention (shared with Types/Function Values): a ```text
                // block immediately after the ```ink block is the expected
                // output.
                if let Some(next) = fences.get(i + 1)
                    && next.lang == "text"
                {
                    assert_eq!(
                        actual.trim_end(),
                        next.body.trim_end(),
                        "example #{ok_examples} output mismatch\n--- source ---\n{}",
                        fences[i].body
                    );
                }
            }
            "ink,error" => {
                error_examples += 1;
                let codes = error_codes(&fences[i].body);
                assert!(
                    codes.contains(&DiagnosticCode::E103),
                    "exceedance example must fail with E103, got {codes:?}\n--- source ---\n{}",
                    fences[i].body
                );
            }
            _ => {}
        }
        i += 1;
    }

    assert!(
        ok_examples >= 4,
        "expected the chapter's worked examples (pure, reads/writes, satisfied \
         assertion, pure sugar) — found {ok_examples}"
    );
    assert!(
        error_examples >= 1,
        "expected the chapter's exceedance (E103) example — found {error_examples}"
    );
}
