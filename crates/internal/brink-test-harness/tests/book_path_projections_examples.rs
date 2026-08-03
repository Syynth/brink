//! Compile-checks the "Path Projections" book chapter's `ink`/`text` example
//! pairs (T1e-3, issue #850).
//!
//! Same shape as `book_function_values_examples.rs` (the "Function Values"
//! chapter's own counterpart, T1c-4 #702): the book's standing rule ("Book
//! code examples must be compile-checked", decision-log 2026-07-10) has no
//! `mdbook test` equivalent for `ink` fences, so this test parses
//! `docs/book/src/toolchain/dialect/path-projections.md`'s fenced code
//! blocks directly out of the shipped file, compiles every ```ink block
//! under `Dialect::Brink`, runs it to completion, and — where the block is
//! immediately followed by a ```text block, the chapter's convention
//! throughout — asserts the output matches.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Step, Story};

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
        .join("path-projections.md")
}

/// One fenced code block: its language tag (`ink`, `text`, …) and body text
/// (exactly the lines between the fences, no trailing fence line).
struct Fence {
    lang: String,
    body: String,
}

/// Extract every fenced code block from markdown, in document order.
/// Intentionally simple (no nested-fence handling — the chapter has none):
/// a line starting with "```" toggles fence state; the language tag is
/// whatever follows "```" on the opening line.
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
            Step::Line(line) => out.push_str(&line.text),
            Step::Done | Step::End | Step::Suspended => {
                break;
            }
            Step::Choices(_) => {
                panic!("chapter example presented choices (must be straight-line):\n{ink_src}");
            }
        }
    }
    out
}

#[test]
fn every_ink_example_in_the_path_projections_chapter_compiles_and_matches_its_output() {
    let markdown = std::fs::read_to_string(chapter_path()).expect("read path-projections.md");
    let fences = extract_fences(&markdown);

    let mut ink_example_count = 0;
    let mut i = 0;
    while i < fences.len() {
        if fences[i].lang == "ink" {
            ink_example_count += 1;
            let actual = run_ink(&fences[i].body);
            // Convention throughout this chapter (and the ones it follows):
            // a ```text block immediately after the ```ink block is the
            // expected output.
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
         (creation, snapshot-at-creation, overlap write-through, #fn, \
         display form) — found {ink_example_count}"
    );
}
