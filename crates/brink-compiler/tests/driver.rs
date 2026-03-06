#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::path::Path;

/// Helper: compile from an in-memory file system (`HashMap` of path to source).
fn compile_mem(
    entry: &str,
    files: &HashMap<&str, &str>,
) -> Result<brink_format::StoryData, brink_compiler::CompileError> {
    brink_compiler::compile(entry, |path| {
        files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {path}"),
            )
        })
    })
}

// ── Single file ─────────────────────────────────────────────────────

#[test]
fn compile_minimal_story() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "Hello, world!\n")]);

    let story = compile_mem("main.ink", &files).unwrap();
    // Codegen is stubbed, so we get an empty StoryData — but the driver
    // ran without errors (parsed, lowered, analyzed).
    assert!(story.containers.is_empty(), "codegen is stubbed");
}

#[test]
fn compile_story_with_knots() {
    let files: HashMap<&str, &str> = HashMap::from([(
        "main.ink",
        "\
Hello!
-> greet

== greet ==
Welcome to the story.
-> END
",
    )]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(story.containers.is_empty(), "codegen is stubbed");
}

// ── INCLUDE discovery ───────────────────────────────────────────────

#[test]
fn compile_follows_includes() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE helpers.ink\nHello!\n-> greet\n"),
        ("helpers.ink", "== greet ==\nWelcome.\n-> END\n"),
    ]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(story.containers.is_empty(), "codegen is stubbed");
}

#[test]
fn compile_nested_includes() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("main.ink", "INCLUDE a.ink\nMain content.\n"),
        ("a.ink", "INCLUDE b.ink\n"),
        ("b.ink", "VAR x = 5\n== knot_b ==\nHello from b.\n-> END\n"),
    ]);

    let story = compile_mem("main.ink", &files).unwrap();
    assert!(story.containers.is_empty(), "codegen is stubbed");
}

#[test]
fn compile_circular_includes_do_not_loop() {
    // Each file includes the other. The driver should visit each once,
    // not loop forever.
    let files: HashMap<&str, &str> = HashMap::from([
        ("a.ink", "INCLUDE b.ink\nContent A.\n"),
        ("b.ink", "INCLUDE a.ink\nContent B.\n"),
    ]);

    let story = compile_mem("a.ink", &files).unwrap();
    assert!(story.containers.is_empty(), "codegen is stubbed");
}

// ── Relative path resolution ────────────────────────────────────────

#[test]
fn compile_resolves_relative_include_paths() {
    let files: HashMap<&str, &str> = HashMap::from([
        ("src/main.ink", "INCLUDE utils/helpers.ink\nHello!\n"),
        ("src/utils/helpers.ink", "== greet ==\nHi.\n-> END\n"),
    ]);

    let story = compile_mem("src/main.ink", &files).unwrap();
    assert!(story.containers.is_empty(), "codegen is stubbed");
}

// ── Error cases ─────────────────────────────────────────────────────

#[test]
fn compile_missing_entry_file() {
    let files: HashMap<&str, &str> = HashMap::new();

    let err = compile_mem("nonexistent.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::Io(_)),
        "expected I/O error for missing entry file, got: {err}"
    );
}

#[test]
fn compile_missing_included_file() {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", "INCLUDE missing.ink\nHello!\n")]);

    let err = compile_mem("main.ink", &files).unwrap_err();
    assert!(
        matches!(err, brink_compiler::CompileError::Io(_)),
        "expected I/O error for missing included file, got: {err}"
    );
}

// ── compile_path (disk-based) ───────────────────────────────────────

#[test]
fn compile_path_reads_from_disk() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tier1/basics/I001-minimal-story/story.ink");

    let story = brink_compiler::compile_path(&path).unwrap();
    assert!(story.containers.is_empty(), "codegen is stubbed");
}

#[test]
fn compile_path_nested_includes_from_disk() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tier3/misc/I025-nested-includes/story.ink");

    let story = brink_compiler::compile_path(&path).unwrap();
    assert!(story.containers.is_empty(), "codegen is stubbed");
}
