//! Tier-1 brink corpus wing (`docs/t1b-surface-spec.md` §6, issue #570).
//!
//! Unlike `tests/tier{1,2,3}/`, this corpus has no C# oracle — vanilla ink
//! never had `~ { … }` blocks, sigil collection literals, or postfix
//! indexing, so there is nothing for inklecate to generate golden episodes
//! from. Each case under `tests/tier1-brink/<name>/` is `story.ink` (brink
//! dialect) plus a hand-written `expected.txt` derived directly from
//! `docs/t1b-surface-spec.md`'s semantics, not from any oracle. This test
//! compiles each case under `Dialect::Brink`, runs it to completion with the
//! deterministic `DotNetRng` (no choices in these cases — straight-line
//! programs are enough to exercise block/loop/collection/RMW lowering), and
//! asserts the concatenated output matches byte-for-byte.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, Line, Story};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("tests")
        .join("tier1-brink")
}

/// Run `story.ink` (brink dialect) to completion and return the concatenated
/// output text. Panics (via `expect`/`unwrap` — test code, exempt per
/// `clippy.toml`) on any compile/runtime error, since every case here is
/// expected to succeed cleanly.
fn run_case(dir: &Path) -> String {
    let ink_path = dir.join("story.ink");
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let compile_msg = format!("compile {}", ink_path.display());
    let output = brink_compiler::compile_path_with_options(&ink_path, options).expect(&compile_msg);
    let link_msg = format!("link {}", ink_path.display());
    let (program, line_tables) = brink_runtime::link(&output.data).expect(&link_msg);
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    let step_msg = format!("runtime error in {}", ink_path.display());
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
    assert!(
        !hit_choices,
        "{} presented choices — tier1-brink cases must be choice-free straight-line programs",
        ink_path.display()
    );
    out
}

fn assert_case(name: &str) {
    let dir = corpus_dir().join(name);
    let expected_msg = format!("read expected.txt for {name}");
    let expected = std::fs::read_to_string(dir.join("expected.txt")).expect(&expected_msg);
    let actual = run_case(&dir);
    assert_eq!(
        actual, expected,
        "case {name}: output mismatch\n--- expected ---\n{expected}\n--- actual ---\n{actual}"
    );
}

#[test]
fn arrays_and_indexing() {
    assert_case("arrays-and-indexing");
}

#[test]
fn while_loop() {
    assert_case("while-loop");
}

#[test]
fn for_in_array() {
    assert_case("for-in-array");
}

#[test]
fn for_in_map_iterates_keys_in_insertion_order() {
    assert_case("for-in-map");
}

#[test]
fn nested_index_assignment_is_rmw() {
    assert_case("nested-index-assignment");
}

#[test]
fn break_and_continue() {
    assert_case("break-continue");
}

#[test]
fn if_else_if_else_chain() {
    assert_case("if-else-chain");
}

/// Every `tests/tier1-brink/` case directory is exercised by a `#[test]`
/// above — a directory with no matching test would silently never run.
#[test]
fn every_case_directory_has_a_test() {
    let known = [
        "arrays-and-indexing",
        "while-loop",
        "for-in-array",
        "for-in-map",
        "nested-index-assignment",
        "break-continue",
        "if-else-chain",
    ];
    let mut found: Vec<String> = std::fs::read_dir(corpus_dir())
        .expect("read tests/tier1-brink")
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    let mut expected: Vec<String> = known.iter().map(|s| (*s).to_string()).collect();
    expected.sort();
    assert_eq!(found, expected, "add a #[test] for every case directory");
}

// ── E054 shadow-warning diagnostic (docs/t1b-surface-spec.md §2) ─────────

#[test]
fn block_scoped_temp_shadowing_an_outer_temp_warns() {
    let source = "~ {\n    temp x = 1\n    if true {\n        temp x = 2\n        x = x + 1\n    }\n}\nDone.\n-> END\n";
    let options = AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    };
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    let out = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        options,
    )
    .expect("shadowing is a warning, not a compile error");
    assert!(
        out.warnings
            .iter()
            .any(|w| w.code == brink_compiler::DiagnosticCode::E054),
        "expected E054 shadow warning, got {:?}",
        out.warnings
    );
}
