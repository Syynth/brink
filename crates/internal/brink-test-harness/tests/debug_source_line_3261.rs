//! Proof tests for issue #3261: the `DebugInfo` file table carries each
//! file's `source_hash` and line index, so the engine can answer
//! `file:line` with **no source text**, and can tell a caller when the text
//! it is measuring against is not the text that was compiled.
//!
//! These drive the production compile road end to end. A round-trip test in
//! `brink-format` proves the fields survive the wire; it cannot prove they
//! hold the *right* values, which is what everything below is for.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::collections::BTreeMap;

use brink_environment::{OptionOverrides, Project};
use brink_runtime::Program;
use brink_source_tree::InMemory;

fn compile_with_debug_info(files: &[(&str, &str)], entry: &str) -> (Program, String) {
    let source = files
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect::<BTreeMap<_, _>>();
    let entry_text = source.get(entry).cloned().expect("entry must be in files");
    let tree = InMemory::new(source);
    let overrides = OptionOverrides {
        debug_info: true,
        ..Default::default()
    };
    let env = Project::load(&tree, entry, &overrides).expect("Project::load");
    let out = brink_environment::compile(&env).expect("compile");
    let (program, _) = brink_runtime::link(&out.data).expect("link");
    (program, entry_text)
}

// ── The line index describes the real file ──────────────────────────────

#[test]
fn line_spans_match_the_actual_lines_of_the_compiled_source() {
    // Irregular line lengths on purpose: a fixed-stride or off-by-one index
    // would survive uniform lines.
    let src = "VAR x = 0\n~ x = 5\n\nhello there friend\n-> END\n";
    let (program, src) = compile_with_debug_info(&[("main.ink", src)], "main.ink");

    // Slice each line out using ONLY what the engine reports, and compare
    // against the real text. This is the assertion that catches an index
    // built from the wrong string or shifted by one.
    let expected: Vec<&str> = src.split_inclusive('\n').collect();
    for (i, want) in expected.iter().enumerate() {
        let line = u32::try_from(i).expect("fits u32");
        let (start, end) = program
            .line_span("main.ink", line)
            .unwrap_or_else(|| unreachable!("line {line} must be in the index"));
        let end = if end == u32::MAX {
            u32::try_from(src.len()).expect("fits u32")
        } else {
            end
        };
        let got = src
            .get(start as usize..end as usize)
            .expect("span must lie inside the source");
        assert_eq!(&got, want, "line {i} span disagrees with the source");
    }

    assert_eq!(
        program.line_span("main.ink", u32::try_from(expected.len()).expect("fits u32")),
        None,
        "a line past the end of the file must not resolve"
    );
}

#[test]
fn a_trailing_newline_does_not_invent_an_extra_line() {
    // `"a\n"` is one line in every editor. An index that pushed a start for
    // the byte after the final newline would report two, and every gutter
    // click below it would be off by one.
    let (program, _) = compile_with_debug_info(&[("main.ink", "~ 1\n-> END\n")], "main.ink");
    assert!(program.line_span("main.ink", 1).is_some(), "line 1 exists");
    assert_eq!(
        program.line_span("main.ink", 2),
        None,
        "the file has two lines; a third must not exist"
    );
}

// ── file:line resolves, with no source text supplied ────────────────────

#[test]
fn resolve_source_line_finds_the_address_without_being_given_source() {
    let src = "VAR x = 0\n~ x = 5\nhello\n-> END\n";
    let (program, src) = compile_with_debug_info(&[("main.ink", src)], "main.ink");

    // Line 1 (0-based) is `~ x = 5`.
    let position = program
        .resolve_source_line("main.ink", 1)
        .expect("an executable line must bind to an address");

    // Cross-check through the OTHER resolver: the address must point back
    // at that line's own text.
    let back = program
        .resolve_debug_position(position)
        .expect("the address must resolve back to source");
    let slice = src
        .get(back.range_start as usize..(back.range_start + back.range_len) as usize)
        .expect("range must lie in the source");
    assert!(
        slice.contains("x = 5"),
        "line 1 resolved to {slice:?}, which is not that line's construct"
    );
}

#[test]
fn resolve_source_line_works_on_the_native_surface_too() {
    let src = "flow main {\n  let v = 7\n  You see it.\n}\n";
    let (program, src) = compile_with_debug_info(&[("main.brink", src)], "main.brink");

    let position = program
        .resolve_source_line("main.brink", 1)
        .expect("`let v = 7` must bind");
    let back = program
        .resolve_debug_position(position)
        .expect("must resolve back");
    let slice = src
        .get(back.range_start as usize..(back.range_start + back.range_len) as usize)
        .expect("range must lie in the source");
    assert!(slice.contains("v = 7"), "resolved to {slice:?}");
}

#[test]
fn a_blank_line_binds_to_nothing_even_though_it_is_in_the_index() {
    // The line EXISTS (it has a span) but holds no code. These are two
    // different answers and a caller needs to tell them apart: "no such
    // line" versus "nothing to break on here".
    let src = "~ temp a = 1\n\n~ temp b = 2\n-> END\n";
    let (program, _) = compile_with_debug_info(&[("main.ink", src)], "main.ink");

    assert!(
        program.line_span("main.ink", 1).is_some(),
        "the blank line is a real line and must have a span"
    );
    assert_eq!(
        program.resolve_source_line("main.ink", 1),
        None,
        "but it holds no executable code, so it must not bind"
    );
    assert!(
        program.resolve_source_line("main.ink", 2).is_some(),
        "control: the next line does bind, so the None above is about the blank line"
    );
}

// ── Staleness detection ─────────────────────────────────────────────────

#[test]
fn source_matches_is_true_for_the_compiled_text_and_false_for_an_edit() {
    let src = "VAR x = 0\n~ x = 5\nhello\n-> END\n";
    let (program, src) = compile_with_debug_info(&[("main.ink", src)], "main.ink");

    assert_eq!(
        program.source_matches("main.ink", &src),
        Some(true),
        "the exact compiled text must match"
    );

    // A one-character edit — the smallest thing that shifts every offset
    // below it, and exactly what the debounce window exposes.
    let edited = src.replace("~ x = 5", "~ x = 6");
    assert_eq!(
        program.source_matches("main.ink", &edited),
        Some(false),
        "edited text must be reported as not matching"
    );

    // Even a change that preserves length and line count.
    let same_shape = src.replace("hello", "hellp");
    assert_eq!(
        program.source_matches("main.ink", &same_shape),
        Some(false),
        "a same-length edit must still be detected"
    );
}

#[test]
fn source_matches_says_cannot_tell_rather_than_no() {
    let (program, _) = compile_with_debug_info(&[("main.ink", "~ 1\n-> END\n")], "main.ink");
    assert_eq!(
        program.source_matches("nope.ink", "whatever"),
        None,
        "an unknown file is `cannot tell`, not `stale` — collapsing them would \
         make every unknown file look permanently dirty"
    );
}

#[test]
fn without_debug_info_there_is_no_line_index_and_no_hash() {
    let out = brink_compiler::compile("main.ink", |_p| Ok("~ 1\n-> END\n".to_owned()))
        .expect("test source compiles");
    let (program, _) = brink_runtime::link(&out.data).expect("link");

    assert_eq!(program.line_span("main.ink", 0), None);
    assert_eq!(program.resolve_source_line("main.ink", 0), None);
    assert_eq!(program.source_matches("main.ink", "~ 1\n-> END\n"), None);
}
