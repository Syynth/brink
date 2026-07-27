//! Integration tests for `brink ide` (epic #289, Phase 1).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const FIXTURE: &str = "\
VAR gold = 0

-> intro

=== intro ===
You have {gold} gold.
~ gold = gold + 5
-> shop

=== shop ===
Welcome. You still have {gold} gold.
-> END
";

/// A project with an unused constant (`UNUSED_MAX`) and an error-free body.
const UNUSED_FIXTURE: &str = "VAR gold = 0\nCONST UNUSED_MAX = 100\n\n-> intro\n\n=== intro ===\nYou have {gold} gold.\n-> END\n";

/// A project with an unresolved divert (a compile error).
const ERR_FIXTURE: &str = "-> nowhere\n\n=== intro ===\nHello.\n-> END\n";

/// A project with an external + call site (for signature) and choices (graph).
const CALL_FIXTURE: &str = "EXTERNAL damage(weapon, amount)\nVAR gold = 0\n\n-> intro\n\n=== intro ===\nYou have {gold} gold.\n~ damage(3, 5)\n* [Browse] -> shop\n+ [Leave] -> END\n\n=== shop ===\nWelcome.\n-> END\n";

/// Write `content` to a unique temp file and return its path.
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn write(tag: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("brink-ide-{}-{}.ink", std::process::id(), tag));
    fs::write(&path, content).unwrap();
    path
}

/// Write the default multi-symbol fixture and return its path.
fn fixture(tag: &str) -> PathBuf {
    write(tag, FIXTURE)
}

fn brink() -> Command {
    Command::new(env!("CARGO_BIN_EXE_brink"))
}

#[test]
fn def_resolves_a_knot() {
    let f = fixture("def");
    let out = brink()
        .args(["ide", "def", "intro", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("knot "), "got: {stdout}");
    assert!(stdout.contains(":5:"), "knot header is on line 5: {stdout}");
    fs::remove_file(&f).ok();
}

#[test]
fn references_counts_all_uses() {
    let f = fixture("refs");
    let out = brink()
        .args(["ide", "references", "gold", "--count", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "4");
    fs::remove_file(&f).ok();
}

#[test]
fn exists_exit_code() {
    let f = fixture("exists");
    // `intro` is referenced by `-> intro` → exit 0.
    let used = brink()
        .args(["ide", "references", "intro", "--exists", "-e"])
        .arg(&f)
        .status()
        .unwrap();
    assert!(used.success());
    fs::remove_file(&f).ok();
}

#[test]
fn unknown_symbol_is_usage_error() {
    let f = fixture("nope");
    let out = brink()
        .args(["ide", "def", "nope", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    fs::remove_file(&f).ok();
}

#[test]
fn json_output_is_parseable() {
    let f = fixture("json");
    let out = brink()
        .args([
            "ide",
            "references",
            "gold",
            "--include-decl",
            "--format",
            "json",
            "-e",
        ])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["count"], 5); // 4 uses + the declaration
    assert_eq!(v["kind"], "variable");
    fs::remove_file(&f).ok();
}

#[test]
fn at_addressing_resolves_cursor_to_definition() {
    let f = fixture("at");
    // Line 7 is `~ gold = gold + 5`; column 3 sits on the first `gold`, a use of
    // the VAR declared on line 1 — def should resolve there.
    let at = format!("{}:7:3", f.display());
    let out = brink()
        .args(["ide", "def", "--at", &at, "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("variable "), "got: {stdout}");
    assert!(
        stdout.contains(":1:"),
        "VAR is declared on line 1: {stdout}"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn symbols_outline_lists_knots_and_globals() {
    let f = fixture("symbols");
    let out = brink()
        .args(["ide", "symbols", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("knot intro"), "got: {s}");
    assert!(s.contains("variable gold"), "got: {s}");
    fs::remove_file(&f).ok();
}

#[test]
fn symbols_search_is_a_flat_name_filter() {
    let f = fixture("search");
    let out = brink()
        .args(["ide", "symbols", "--search", "gold", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("gold"), "got: {s}");
    assert!(
        !s.contains("knot"),
        "search is flat, only gold matches: {s}"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn unused_reports_dead_symbols_and_exits_nonzero() {
    let f = write("unused", UNUSED_FIXTURE);
    let out = brink()
        .args(["ide", "unused", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("UNUSED_MAX"),
        "the unused constant should be reported"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn check_clean_project_is_silent_and_zero() {
    let f = fixture("check-ok");
    let out = brink()
        .args(["ide", "check", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "clean project emits nothing");
    fs::remove_file(&f).ok();
}

#[test]
fn check_reports_errors_and_exits_nonzero() {
    let f = write("check-err", ERR_FIXTURE);
    let out = brink()
        .args(["ide", "check", "--format", "json", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["severity"], "error");
    assert!(v[0]["code"].as_str().unwrap().starts_with('E'));
    fs::remove_file(&f).ok();
}

#[test]
fn rename_preview_lists_edits_without_writing() {
    let f = fixture("rn-preview");
    let out = brink()
        .args(["ide", "rename", "gold", "--to", "coins", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("gold -> coins"),
        "preview should list edits"
    );
    assert!(
        fs::read_to_string(&f).unwrap().contains("VAR gold"),
        "preview must not touch the file"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn rename_write_applies_a_safe_rename() {
    let f = fixture("rn-write");
    let out = brink()
        .args(["ide", "rename", "gold", "--to", "coins", "--write", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let src = fs::read_to_string(&f).unwrap();
    assert!(
        src.contains("VAR coins"),
        "the declaration is renamed: {src}"
    );
    assert!(src.contains("{coins}"), "references are renamed: {src}");
    fs::remove_file(&f).ok();
}

#[test]
fn rename_refuses_a_change_that_introduces_a_diagnostic() {
    let f = fixture("rn-collide");
    // intro -> shop collides with the existing `shop` knot (a duplicate warning).
    let out = brink()
        .args(["ide", "rename", "intro", "--to", "shop", "--write", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        fs::read_to_string(&f).unwrap().contains("=== intro ==="),
        "the file must be left unchanged"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn rename_unsafe_overrides_the_safety_gate() {
    let f = fixture("rn-unsafe");
    let out = brink()
        .args([
            "ide", "rename", "intro", "--to", "shop", "--write", "--unsafe", "-e",
        ])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(
        !fs::read_to_string(&f).unwrap().contains("=== intro ==="),
        "--unsafe applies the rename anyway"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn rename_patch_is_a_git_applyable_diff() {
    let f = fixture("rn-patch");
    let out = brink()
        .args(["ide", "rename", "gold", "--to", "coins", "--patch", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("diff --git"), "git header: {s}");
    assert!(s.contains("@@ "), "a hunk: {s}");
    assert!(s.contains("+VAR coins = 0"), "the renamed line: {s}");
    assert!(
        fs::read_to_string(&f).unwrap().contains("VAR gold"),
        "patch mode must not write"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn hover_describes_a_symbol() {
    let f = fixture("hover");
    let out = brink()
        .args(["ide", "hover", "gold", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("variable"), "got: {s}");
    assert!(s.contains("gold"), "got: {s}");
    fs::remove_file(&f).ok();
}

#[test]
fn signature_at_a_call_shows_params() {
    let f = write("sig", CALL_FIXTURE);
    // Line 8 is `~ damage(3, 5)`; column 10 sits on the first argument.
    let at = format!("{}:8:10", f.display());
    let out = brink()
        .args(["ide", "signature", "--at", &at, "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("damage("), "the signature label: {s}");
    assert!(s.contains("weapon"), "the active parameter: {s}");
    fs::remove_file(&f).ok();
}

#[test]
fn graph_text_and_dot_render() {
    let f = write("graph", CALL_FIXTURE);
    let text = brink()
        .args(["ide", "graph", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(text.status.success());
    let s = String::from_utf8(text.stdout).unwrap();
    assert!(s.contains("knot intro"), "node listed: {s}");
    assert!(s.contains("->"), "an edge listed: {s}");

    let dot = brink()
        .args(["ide", "graph", "--dot", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    let d = String::from_utf8(dot.stdout).unwrap();
    assert!(d.starts_with("digraph story {"), "DOT header: {d}");
    fs::remove_file(&f).ok();
}

#[test]
fn graph_json_is_parseable() {
    let f = write("graph-json", CALL_FIXTURE);
    let out = brink()
        .args(["ide", "graph", "--format", "json", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["nodes"].is_array(), "has nodes: {v}");
    assert!(v["edges"].is_array(), "has edges: {v}");
    fs::remove_file(&f).ok();
}

#[test]
fn lines_classifies_each_line() {
    let f = write("lines", CALL_FIXTURE);
    let out = brink()
        .args(["ide", "lines", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("KnotHeader"), "classifies knot headers: {s}");
    assert!(s.contains("External"), "classifies the external decl: {s}");
    fs::remove_file(&f).ok();
}

// ── Phase 3b: refactors, move-file, actions ─────────────────────────

/// A project with knots out of alphabetical order, a stitch + a self-reference
/// (`-> intro.evidence`) to exercise sort / reorder / promote / convert.
const REFAC: &str = "\
-> intro

=== zebra ===
Z.
-> intro

=== intro ===
Hi.
* [x] -> intro.evidence
= evidence
A clue. -> intro.evidence
-> END

=== apple ===
A.
-> END
";

/// Create a multi-file project under a fresh temp dir; return the dir.
#[expect(clippy::unwrap_used, reason = "test fixture setup")]
fn project(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("brink-ide-proj-{}-{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    for (rel, content) in files {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }
    dir
}

#[test]
fn refactor_sort_knots_previews_a_diff() {
    let f = write("rf-sort", REFAC);
    let out = brink()
        .args(["ide", "refactor", "sort-knots", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("diff --git"), "a git diff: {s}");
    // apple sorts before intro/zebra: the `=== apple ===` header is added early.
    assert!(s.contains("+=== apple ==="), "apple moves up: {s}");
    // preview never writes.
    assert!(
        fs::read_to_string(&f).unwrap().starts_with("-> intro"),
        "unchanged"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn refactor_reorder_stitch_needs_qualified_name() {
    let f = write("rf-ro-bad", REFAC);
    // A bare knot name is not a KNOT.STITCH — usage error (exit 2).
    let out = brink()
        .args(["ide", "refactor", "reorder-stitch", "intro", "up", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    fs::remove_file(&f).ok();
}

#[test]
fn refactor_promote_preview_requalifies_same_file_refs() {
    // `evidence` is referenced by qualified name within the same file
    // (`-> intro.evidence`) and by a self-divert inside its own body. Promote
    // folds the requalified references into the new source, so the preview shows
    // the promotion AND the rewritten bare `-> evidence` references — and
    // introduces no new diagnostics.
    let f = write("rf-promote-prev", REFAC);
    let out = brink()
        .args(["ide", "refactor", "promote-stitch", "intro.evidence", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success(), "preview is never gated");
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(
        s.contains("+=== evidence ==="),
        "the promotion is shown: {s}"
    );
    assert!(
        s.contains("+* [x] -> evidence"),
        "the external same-file ref is requalified to bare: {s}"
    );
    assert!(
        s.contains("+A clue. -> evidence"),
        "the in-stitch self-divert is requalified to bare: {s}"
    );
    assert!(
        !s.contains("would introduce"),
        "no dangling reference, so no breakage is surfaced: {s}"
    );
    // preview must not write.
    assert!(
        fs::read_to_string(&f).unwrap().contains("= evidence"),
        "unchanged"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn refactor_promote_writes_cleanly_without_unsafe() {
    // Promote now updates same-file references, so it introduces no new
    // diagnostics and the safety gate lets a plain `--write` through.
    let f = write("rf-promote-gate", REFAC);
    let out = brink()
        .args([
            "ide",
            "refactor",
            "promote-stitch",
            "intro.evidence",
            "--write",
            "-e",
        ])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "a non-breaking promote writes without --unsafe"
    );
    let written = fs::read_to_string(&f).unwrap();
    assert!(
        written.contains("=== evidence ==="),
        "the stitch was promoted: {written}"
    );
    assert!(
        !written.contains("intro.evidence"),
        "no dangling qualified reference remains in the written file: {written}"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn refactor_convert_line_changes_a_lines_sigil() {
    let f = write("rf-convert", REFAC);
    // Line 8 is the narrative `Hi.`; convert it to a choice.
    let at = format!("{}:8:1", f.display());
    let out = brink()
        .args([
            "ide",
            "refactor",
            "convert-line",
            "--at",
            &at,
            "choice",
            "-e",
        ])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("-Hi."), "old narrative line removed: {s}");
    assert!(
        s.lines()
            .any(|l| l.starts_with('+') && l.contains("Hi.") && l.contains('*')),
        "new line is a choice: {s}"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn refactor_reorder_knots_write_applies_a_permutation() {
    let f = write("rf-perm", REFAC);
    let out = brink()
        .args([
            "ide",
            "refactor",
            "reorder-knots",
            "apple,intro,zebra",
            "--write",
            "-e",
        ])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let src = fs::read_to_string(&f).unwrap();
    let apple = src.find("=== apple ===").unwrap();
    let zebra = src.find("=== zebra ===").unwrap();
    assert!(apple < zebra, "apple now precedes zebra: {src}");
    fs::remove_file(&f).ok();
}

#[test]
fn actions_lists_available_refactors() {
    let f = write("rf-actions", REFAC);
    // Cursor on the `intro` knot header (line 7) offers a Format-knot action.
    let at = format!("{}:7:5", f.display());
    let out = brink()
        .args(["ide", "actions", "--at", &at, "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(
        s.to_lowercase().contains("intro"),
        "an action names the knot: {s}"
    );
    fs::remove_file(&f).ok();
}

#[test]
fn move_file_previews_rename_and_include_rewrite() {
    let dir = project(
        "mv-prev",
        &[
            ("main.ink", "INCLUDE scenes/intro.ink\n\n-> intro\n"),
            ("scenes/intro.ink", "=== intro ===\nHello.\n-> END\n"),
        ],
    );
    let out = brink()
        .current_dir(&dir)
        .args([
            "ide",
            "move-file",
            "scenes/intro.ink",
            "scenes/act1/intro.ink",
            "-e",
            "main.ink",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(
        s.contains("rename from scenes/intro.ink"),
        "git rename header: {s}"
    );
    assert!(
        s.contains("+INCLUDE scenes/act1/intro.ink"),
        "inbound INCLUDE rewritten: {s}"
    );
    // preview never touches disk.
    assert!(
        dir.join("scenes/intro.ink").exists(),
        "preview must not move the file"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn move_file_write_relocates_and_rewrites_includes() {
    let dir = project(
        "mv-write",
        &[
            ("main.ink", "INCLUDE scenes/intro.ink\n\n-> intro\n"),
            ("scenes/intro.ink", "=== intro ===\nHello.\n-> END\n"),
        ],
    );
    let out = brink()
        .current_dir(&dir)
        .args([
            "ide",
            "move-file",
            "scenes/intro.ink",
            "scenes/act1/intro.ink",
            "--write",
            "-e",
            "main.ink",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("scenes/act1/intro.ink").exists(), "file relocated");
    assert!(!dir.join("scenes/intro.ink").exists(), "old path gone");
    assert!(
        fs::read_to_string(dir.join("main.ink"))
            .unwrap()
            .contains("scenes/act1/intro.ink"),
        "INCLUDE rewritten on disk"
    );
    // The relocated project still analyzes clean.
    let chk = brink()
        .current_dir(&dir)
        .args(["ide", "check", "-e", "main.ink"])
        .status()
        .unwrap();
    assert!(chk.success(), "clean after move");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn move_file_json_preview_has_the_mutation_shape() {
    let dir = project(
        "mv-json",
        &[
            ("main.ink", "INCLUDE scenes/intro.ink\n\n-> intro\n"),
            ("scenes/intro.ink", "=== intro ===\nHello.\n-> END\n"),
        ],
    );
    let out = brink()
        .current_dir(&dir)
        .args([
            "ide",
            "move-file",
            "scenes/intro.ink",
            "scenes/act1/intro.ink",
            "--format",
            "json",
            "-e",
            "main.ink",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["diff"].is_string(), "diff present: {v}");
    assert!(v["files"].is_array(), "files present: {v}");
    assert_eq!(v["safe"], true, "clean move is safe: {v}");
    assert!(v["introducedDiagnostics"].as_array().unwrap().is_empty());
    fs::remove_dir_all(&dir).ok();
}

// ── native (`.brink`) discovery via `discover_native` (issue #1295) ─────

/// Create a *nested* native project: a `brink.toml` lives inside `story/`
/// (not at `dir`, the process cwd used below), so
/// `native_source_root("story/main.brink")` resolves to `dir/story` — a
/// root that is *not* the cwd. `discover_native` keys every file
/// root-relative to that root (e.g. `"main.brink"`, `"other.brink"`), so an
/// fs-write site that writes a key literally (instead of resolving it back
/// against the root) lands on a phantom `dir/main.brink` instead of the
/// real `dir/story/main.brink` — the #1295 regression this module's tests
/// guard against.
fn nested_native_project(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let mut all = vec![("story/brink.toml", "[project]\n")];
    all.extend(files.iter().copied());
    project(tag, &all)
}

/// Regression: `rename --write` on a nested native entry (`native_source_root
/// != cwd`) must write the real file under the source root, not a
/// cwd-relative phantom of the same bare key.
#[test]
fn rename_write_on_nested_native_entry_writes_the_real_file() {
    let dir = nested_native_project(
        "rn-native-nested",
        &[(
            "story/main.brink",
            "var gold = 0\n\nflow main() {\n  You have {gold} gold. -> END\n}\n",
        )],
    );
    let out = brink()
        .current_dir(&dir)
        .args(["ide", "rename", "gold", "--to", "coins", "--write", "-e"])
        .arg("story/main.brink")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let real = dir.join("story/main.brink");
    let phantom = dir.join("main.brink");
    assert!(
        !phantom.exists(),
        "must not write a cwd-relative phantom file at {}",
        phantom.display()
    );
    assert!(
        real.exists(),
        "the real file must exist at {}",
        real.display()
    );
    let src = fs::read_to_string(&real).unwrap();
    assert!(src.contains("var coins"), "declaration renamed: {src}");
    assert!(src.contains("{coins}"), "reference renamed: {src}");
    fs::remove_dir_all(&dir).ok();
}

/// Regression: `move-file --write` on a nested native project (`
/// native_source_root != cwd`) must relocate the real file under the source
/// root, not a cwd-relative phantom of the bare `old`/`new` keys.
#[test]
fn move_file_write_on_nested_native_entry_relocates_the_real_file() {
    let dir = nested_native_project(
        "mv-native-nested",
        &[
            ("story/main.brink", "flow main() {\n  Hello. -> END\n}\n"),
            ("story/other.brink", "flow other() {\n  Hi. -> END\n}\n"),
        ],
    );
    let out = brink()
        .current_dir(&dir)
        .args([
            "ide",
            "move-file",
            "other.brink",
            "moved/other.brink",
            "--write",
            "-e",
        ])
        .arg("story/main.brink")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        dir.join("story/moved/other.brink").exists(),
        "the real file relocated under the source root"
    );
    assert!(
        !dir.join("story/other.brink").exists(),
        "the old real path is gone"
    );
    assert!(
        !dir.join("moved/other.brink").exists(),
        "must not write a cwd-relative phantom file"
    );
    assert!(
        !dir.join("other.brink").exists(),
        "must not leave a cwd-relative phantom of the old key either"
    );

    // The relocated project still discovers + analyzes clean.
    let chk = brink()
        .current_dir(&dir)
        .args(["ide", "check", "-e", "story/main.brink"])
        .status()
        .unwrap();
    assert!(chk.success(), "clean after move");
    fs::remove_dir_all(&dir).ok();
}

/// Regression: every `brink ide` subcommand (not just `effects-diff --rev`,
/// #1224's fix) must discover a multi-file native project's *whole* file
/// set via `discover_native`, not just the entry — `symbols --search`
/// searches `project.analysis`, which only spans what got discovered.
#[test]
fn ide_subcommand_on_two_file_native_project_sees_both_files_symbols() {
    let dir = project(
        "native-two-file-symbols",
        &[
            ("main.brink", "flow main() {\n  Hello. -> END\n}\n"),
            ("other.brink", "flow other() {\n  Hi. -> END\n}\n"),
        ],
    );
    let out = brink()
        .current_dir(&dir)
        .args(["ide", "symbols", "--search", "", "-e", "main.brink"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8(out.stdout).unwrap();
    assert!(s.contains("main"), "entry file's flow is found: {s}");
    assert!(
        s.contains("other"),
        "the sibling file's flow must also be discovered (not just the entry): {s}"
    );
    fs::remove_dir_all(&dir).ok();
}

// ── effects-diff (T2-4, #863, docs/effects-spec.md §10) ─────────────────

/// A baseline project: `spend` reads + writes `gold`.
const EFFECTS_BASE: &str =
    "VAR gold = 10\n-> spend\n\n=== spend ===\n~ gold = gold - 1\nSpent.\n-> END\n";

/// The same project after an edit: `spend` now also reads + writes `silver`.
const EFFECTS_HEAD: &str = "VAR gold = 10\nVAR silver = 5\n-> spend\n\n=== spend ===\n~ gold = gold - 1\n~ silver = silver + 1\nSpent.\n-> END\n";

#[test]
fn effects_diff_base_reports_a_changed_row_as_markdown() {
    let base = write("effdiff-base", EFFECTS_BASE);
    let head = write("effdiff-head", EFFECTS_HEAD);
    let out = brink()
        .args(["ide", "effects-diff", "--base"])
        .arg(&base)
        .arg("-e")
        .arg(&head)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("## Effect row diff"), "{stdout}");
    assert!(stdout.contains("**knot spend** — changed"), "{stdout}");
    assert!(stdout.contains("silver"), "{stdout}");
    fs::remove_file(&base).ok();
    fs::remove_file(&head).ok();
}

#[test]
fn effects_diff_json_shape() {
    let base = write("effdiff-json-base", EFFECTS_BASE);
    let head = write("effdiff-json-head", EFFECTS_HEAD);
    let out = brink()
        .args(["ide", "effects-diff", "--format", "json", "--base"])
        .arg(&base)
        .arg("-e")
        .arg(&head)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["changed"], 1, "{v}");
    assert_eq!(v["added"], 0, "{v}");
    assert_eq!(v["removed"], 0, "{v}");
    let entry = &v["entries"][0];
    assert_eq!(entry["def"], "knot spend", "{v}");
    assert_eq!(entry["change"], "changed", "{v}");
    assert!(
        entry["head"]["writes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "silver"),
        "{v}"
    );
    fs::remove_file(&base).ok();
    fs::remove_file(&head).ok();
}

#[test]
fn effects_diff_exit_code_flag_fails_on_change_and_passes_when_identical() {
    let base = write("effdiff-ec-base", EFFECTS_BASE);
    let head = write("effdiff-ec-head", EFFECTS_HEAD);
    // --exit-code: nonzero when rows moved.
    let changed = brink()
        .args(["ide", "effects-diff", "--exit-code", "--base"])
        .arg(&base)
        .arg("-e")
        .arg(&head)
        .output()
        .unwrap();
    assert_eq!(changed.status.code(), Some(1), "changed rows must exit 1");
    // Identical projects: no change, exit 0, reassuring message.
    let same = brink()
        .args(["ide", "effects-diff", "--exit-code", "--base"])
        .arg(&head)
        .arg("-e")
        .arg(&head)
        .output()
        .unwrap();
    assert!(same.status.success(), "identical rows must exit 0");
    assert!(
        String::from_utf8(same.stdout)
            .unwrap()
            .contains("No effect row changes"),
        "identical projects report no changes"
    );
    fs::remove_file(&base).ok();
    fs::remove_file(&head).ok();
}

#[test]
fn effects_diff_requires_a_baseline_selector() {
    let head = write("effdiff-nobaseline", EFFECTS_HEAD);
    let out = brink()
        .args(["ide", "effects-diff", "-e"])
        .arg(&head)
        .output()
        .unwrap();
    // No --rev / --base → usage error (exit 2), not a panic.
    assert_eq!(
        out.status.code(),
        Some(2),
        "missing baseline is a usage error"
    );
    fs::remove_file(&head).ok();
}

// ── Issue #1539 review finding: `def --at` on a UFCS call site ─────────

/// 1-based `(line, col)` of `byte_offset` within `src` (matches `--at`'s own
/// 1-based convention).
fn line_col(src: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in src.char_indices() {
        if i == byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Regression for a review finding on #1539/PR #1543: no test covered the
/// `brink ide def --at` surface, which the issue names as bug #1 and where
/// the fix actually lives (`Project::resolve`,
/// `crates/brink-cli/src/ide/project.rs`). Every other new test added for
/// #1539 exercises `brink-ide`'s `navigation::find_references`/`rename::*`
/// directly; none touch `Project::resolve`, so the new CLI UFCS block was
/// unproven. Uses the existing `nested_native_project` native-project
/// fixture, per the finding's own suggestion.
#[test]
fn at_addressing_on_a_ufcs_call_site_resolves_to_the_free_function() {
    const SRC: &str = "\
struct Guest {
  name: string
}

fn greet(g, loudness) {
  return loudness;
}

fn main() {
  let g = Guest { name: \"ada\" };
  let n = g.greet(3);
}
";
    let dir = nested_native_project("def-at-ufcs", &[("story/main.brink", SRC)]);

    let call_byte = SRC.find("greet(3)").expect("call site");
    let (line, col) = line_col(SRC, call_byte);
    // Native discovery keys files root-relative to `native_source_root`
    // (here, `dir/story`), not by absolute filesystem path — `--at`'s FILE
    // component must match that key (`"main.brink"`), same as
    // `db.file_id` expects (issue #1295's root-relative convention).
    let at = format!("main.brink:{line}:{col}");

    let out = brink()
        .current_dir(&dir)
        .args(["ide", "def", "--at", &at, "-e"])
        .arg("story/main.brink")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();

    let decl_byte = SRC.find("greet(g").expect("decl");
    let (decl_line, _) = line_col(SRC, decl_byte);
    let receiver_byte = SRC.find("g = Guest").expect("receiver decl");
    let (receiver_line, _) = line_col(SRC, receiver_byte);

    assert!(
        stdout.contains(&format!(":{decl_line}:")),
        "must jump to the `fn greet` declaration on line {decl_line}, got: {stdout}"
    );
    assert!(
        !stdout.contains(&format!(":{receiver_line}:")),
        "must NOT jump to the receiver `g`'s own declaration on line {receiver_line} — the \
         #1539 bug this guards against: {stdout}"
    );
    fs::remove_dir_all(&dir).ok();
}

// ── brink ide: --deny/--warn/--allow (issue #1417) ────────────────────
//
// Extends the CLI/API lint-override tier `brink compile` gained in #1373
// (`crates/brink-cli/tests/project_config_cli.rs`) to `brink ide`. Reuses
// the identical `E014_FIXTURE` (a no-op `~` logic line, `Warning` by
// default) so a passing/failing `check` is a direct black-box signal of
// whether the override actually reached this surface's `AnalysisOptions`.

/// A logic line with no effect (`~` alone) — `DiagnosticCode::E014`,
/// `Warning` by default. Plain `strict-ink` source, no extension syntax, so
/// only the lint-override tier can make `check` fail on it.
const E014_FIXTURE: &str = "Hello.\n~\n-> END\n";

#[test]
fn ide_check_no_lint_flags_e014_stays_a_warning() {
    let f = write("ide-check-e014-default", E014_FIXTURE);
    let out = brink()
        .args(["ide", "check", "-e"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "with no --deny/-D warnings flag, E014 must stay a Warning and `check` must exit 0: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    fs::remove_file(&f).ok();
}

#[test]
fn ide_check_deny_e014_flag_promotes_the_warning_to_an_error() {
    let f = write("ide-check-e014-deny", E014_FIXTURE);
    let out = brink()
        .args(["ide", "check", "--format", "json", "-e"])
        .arg(&f)
        .args(["--deny", "E014"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "--deny E014 must make an ordinarily-Warning diagnostic fail `ide check`: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["severity"], "error");
    assert_eq!(v[0]["code"], "E014");
    fs::remove_file(&f).ok();
}

#[test]
fn ide_check_short_deny_warnings_flag_promotes_e014_to_an_error() {
    let f = write("ide-check-e014-dw", E014_FIXTURE);
    let out = brink()
        .args(["ide", "check", "--format", "json", "-e"])
        .arg(&f)
        .args(["-D", "warnings"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "-D warnings must promote every Warning (including E014) to an error: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["severity"], "error");
    fs::remove_file(&f).ok();
}

/// `--deny` must win over a conflicting `brink.toml` `[lints] E014 =
/// "allow"` for the same code (#1005 `CLI/API > file > default`
/// precedence) — proves the override is applied *after* the file, not
/// merely alongside it.
#[test]
fn ide_check_deny_flag_wins_over_a_conflicting_brink_toml_allow() {
    let dir = std::env::temp_dir().join(format!(
        "brink-ide-cli-deny-wins-over-file-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("brink.toml"), "[lints]\nE014 = \"allow\"\n").unwrap();
    fs::write(dir.join("story.ink"), E014_FIXTURE).unwrap();

    let out = brink()
        .args(["ide", "check", "--format", "json", "-e"])
        .arg(dir.join("story.ink"))
        .args(["--deny", "E014"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "--deny E014 must win over the file's `[lints] E014 = \"allow\"`: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v[0]["severity"], "error");
    fs::remove_dir_all(&dir).ok();
}

/// `--allow` must win over a conflicting `brink.toml` `[lints] E014 =
/// "deny"` entry, the reverse direction of the above — the precedent this
/// PR mirrors (`crates/brink-cli/tests/project_config_cli.rs`'s
/// `compile_allow_flag_overrides_a_conflicting_brink_toml_deny`) covers this
/// direction for `brink compile`; `brink ide check` had none. Also passes
/// `-D warnings` alongside `--allow E014`: `Allow` is a distinct branch of
/// `effective_severity` (step 3) that short-circuits before `deny_warnings`
/// (step 4) is ever consulted, so `--allow` must win even when
/// `deny-warnings` is also in effect for this code.
#[test]
fn ide_check_allow_flag_wins_over_a_conflicting_brink_toml_deny() {
    let dir = std::env::temp_dir().join(format!(
        "brink-ide-cli-allow-wins-over-file-{}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("brink.toml"), "[lints]\nE014 = \"deny\"\n").unwrap();
    fs::write(dir.join("story.ink"), E014_FIXTURE).unwrap();

    // Sanity check: the file alone denies E014 and fails `check`.
    let baseline = brink()
        .args(["ide", "check", "-e"])
        .arg(dir.join("story.ink"))
        .output()
        .unwrap();
    assert_eq!(
        baseline.status.code(),
        Some(1),
        "sanity check: brink.toml's E014 = \"deny\" alone must fail `ide check`: {}",
        String::from_utf8_lossy(&baseline.stderr)
    );

    let out = brink()
        .args(["ide", "check", "--format", "json", "-e"])
        .arg(dir.join("story.ink"))
        .args(["--allow", "E014"])
        .args(["-D", "warnings"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "--allow E014 must win over the file's `[lints] E014 = \"deny\"`, and stay immune to \
         -D warnings: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    fs::remove_dir_all(&dir).ok();
}
