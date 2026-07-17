//! Compile-checks the "Effects" book chapter's `ink`/`text` example pairs
//! (T2-4, docs/effects-spec.md §10, issue #863 — tracked from #859).
//!
//! The book's standing rule ("Book code examples must be compile-checked",
//! decision-log 2026-07-10) covers Rust doctests via `mdbook test`; `mdbook`
//! itself has no equivalent for `ink` fences. This is the ink-side
//! counterpart for the new "Effects" chapter — the same shape as
//! `book_function_values_examples.rs`/`book_path_projections_examples.rs`,
//! which established the convention this chapter follows: it parses
//! `docs/book/src/toolchain/dialect/effects.md`'s fenced code blocks
//! directly out of the shipped file (not a copy embedded in this test),
//! compiles every ```ink block under `Dialect::Brink`, runs it to
//! completion, and — where the block is immediately followed by a ```text
//! block — asserts the output matches. An edit to the chapter that breaks
//! an example fails this test, the same way an edit to a Rust doctest fails
//! `mdbook test`.
//!
//! `#@effects(…)` exceedance (the one compile *error* this surface
//! produces) is deliberately **not** demonstrated as a fenced `ink` block —
//! same call the chapter itself makes for other compile-error cases (see
//! "Path Projections"'s prose-only treatment): a block tagged `ink` here is
//! a contract that it compiles and runs clean, so a deliberately-failing
//! example is prose instead, with the exact diagnostic message quoted
//! inline. `crates/internal/brink-test-harness/tests/tier1_brink.rs`'s T2-4
//! section and `crates/internal/brink-db/tests/t2_2_effects_assertions.rs`
//! cover the exceedance error end-to-end instead.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use brink_compiler::{AnalysisOptions, Dialect};
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

/// One fenced code block: its language tag (`ink`, `text`, …) and body text
/// (exactly the lines between the fences, no trailing fence line).
struct Fence {
    lang: String,
    body: String,
}

/// Extract every fenced code block from markdown, in document order.
/// Intentionally simple (no nested-fence handling — the chapter has none):
/// a line starting with a triple-backtick fence toggles fence state; the
/// language tag is whatever follows it on the opening line. A fence
/// indented under a list item (the shell CLI examples near the end of
/// this chapter) never starts at column 0, so it's naturally excluded —
/// this only ever picks up the top-level `ink`/`text` pairs.
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

/// Compile `ink_src` (brink dialect) and run it to completion, returning the
/// concatenated output text. Panics on any compile/runtime error or on
/// hitting a choice — every example in this chapter is a choice-free
/// straight-line program (tier1-brink corpus convention).
fn run_ink(ink_src: &str) -> String {
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", ink_src)]);
    let output = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        options,
    )
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
                panic!("chapter example presented choices (must be straight-line):\n{ink_src}");
            }
        }
    }
    out
}

#[test]
fn every_ink_example_in_the_effects_chapter_compiles_and_matches_its_output() {
    let markdown = std::fs::read_to_string(chapter_path()).expect("read effects.md");
    let fences = extract_fences(&markdown);

    let mut ink_example_count = 0;
    let mut i = 0;
    while i < fences.len() {
        if fences[i].lang == "ink" {
            ink_example_count += 1;
            let actual = run_ink(&fences[i].body);
            // Convention throughout the dialect chapters: a ```text block
            // immediately after the ```ink block is the expected output.
            if let Some(next) = fences.get(i + 1)
                && next.lang == "text"
            {
                assert_eq!(
                    actual.trim_end(),
                    next.body.trim_end(),
                    "example #{ink_example_count} output mismatch\n--- source ---\n{}",
                    fences[i].body
                );
            }
        }
        i += 1;
    }

    assert!(
        ink_example_count >= 5,
        "expected the chapter to still have its worked ink examples \
         (a row's atoms, rows composing, opaque function-value calls, \
         asserting a bound, and `pure`) — found {ink_example_count}"
    );
}
