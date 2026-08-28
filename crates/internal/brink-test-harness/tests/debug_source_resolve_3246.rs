//! Proof tests for issue #3246: the **inverse** resolver —
//! [`brink_runtime::Program::resolve_source_range`], source span → program
//! address. D9 (#3187) built program → source; this is the half a
//! breakpoint gutter needs, because `BreakpointSet` is keyed by
//! `(container_idx, offset)` while an editor speaks in source.
//!
//! Proof shape follows D9's own: every assertion is cross-checked against
//! the raw source text rather than the resolver's own output alone, and
//! both surfaces (`.ink` and `.brink`) are covered per the RULED
//! both-surfaces requirement (`docs/debugger-spec.md` §0).
//!
//! The load-bearing test here is the **round trip**: an address the inverse
//! resolver returns must resolve *back* through `resolve_debug_position` to
//! a range inside the span asked for. Two independently-written indexes
//! over the same table are exactly where an off-by-one hides, and neither
//! direction alone can catch it.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::collections::BTreeMap;

use brink_environment::{OptionOverrides, Project};
use brink_runtime::Program;
use brink_source_tree::InMemory;

/// Compile with debug info over the production road, returning the linked
/// program alongside the entry's own source text.
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

/// The half-open byte range of the line (0-based) containing `needle`, the
/// way an editor would hand one over — this is deliberately computed from
/// the source text here, not by the runtime, because the runtime has no
/// line table and that is the whole reason the API takes bytes.
fn line_range_containing(src: &str, needle: &str) -> (u32, u32) {
    let at = src.find(needle).expect("needle must appear in source");
    let start = src[..at].rfind('\n').map_or(0, |nl| nl + 1);
    let end = src[at..].find('\n').map_or(src.len(), |nl| at + nl + 1);
    (
        u32::try_from(start).expect("fits u32"),
        u32::try_from(end).expect("fits u32"),
    )
}

// ── The round trip: the two resolvers must agree ────────────────────────

fn round_trips_on(files: &[(&str, &str)], entry: &str, needle: &str) {
    let (program, src) = compile_with_debug_info(files, entry);
    let (start, end) = line_range_containing(&src, needle);

    let bound = program.resolve_source_range(entry, start, end);
    assert!(
        bound.is_some(),
        "`{needle}` is executable code and must bind to an address"
    );
    let position = bound.expect("just asserted above");

    // Back the other way. The range we land on must lie inside the span we
    // asked about — otherwise one of the two indexes is off and a gutter
    // would arm a breakpoint on a line the user did not click.
    let back = program
        .resolve_debug_position(position)
        .expect("an address the inverse resolver produced must resolve back");
    assert_eq!(back.file.as_deref(), Some(entry));
    assert!(
        back.range_start >= start && back.range_start < end,
        "round trip escaped the span: asked [{start}, {end}) for `{needle}`, came back at \
         {} (which slices to {:?})",
        back.range_start,
        src.get(back.range_start as usize..(back.range_start + back.range_len) as usize),
    );

    // And the text it names is really the construct asked for.
    let slice = src
        .get(back.range_start as usize..(back.range_start + back.range_len) as usize)
        .expect("recorded range must lie inside the source it names");
    assert!(
        needle.contains(slice.trim()) || slice.contains(needle.trim()),
        "resolved to {slice:?}, which is not the `{needle}` construct asked for"
    );
}

#[test]
fn source_range_round_trips_to_the_same_line_ink() {
    round_trips_on(
        &[("main.ink", "VAR x = 0\n~ x = 5\nhello\n-> END\n")],
        "main.ink",
        "~ x = 5",
    );
}

#[test]
fn source_range_round_trips_to_the_same_line_native() {
    round_trips_on(
        &[("main.brink", "flow main {\n  let v = 7\n  You see it.\n}\n")],
        "main.brink",
        "let v = 7",
    );
}

// ── `None` is an answer: a span with no code must refuse, not guess ──────

#[test]
fn a_comment_line_binds_to_nothing_rather_than_to_the_nearest_code() {
    // The comment sits between two executable lines, and the line BEFORE
    // it must itself carry a debug entry — otherwise this fixture proves
    // nothing. (First draft used `VAR x = 0` there, which emits no entry in
    // this file: an implementation that dropped the span's lower bound and
    // widened onto earlier lines passed all seven tests. Caught by probe,
    // not by review.) A resolver that widened to "nearest entry" instead of
    // "inside this span" now returns the *preceding* assignment's address,
    // arming a breakpoint the user never asked for, on a line they cannot
    // watch execute.
    let src = "~ temp before = 1\n// just a note\n~ temp after = 2\n-> END\n";
    let (program, src) = compile_with_debug_info(&[("main.ink", src)], "main.ink");
    let (start, end) = line_range_containing(&src, "// just a note");

    assert_eq!(
        program.resolve_source_range("main.ink", start, end),
        None,
        "a comment line must bind to nothing — a breakpoint that can never \
         hit is worse than a gutter that visibly refuses"
    );

    // Control: the very next line DOES bind, so the `None` above is about
    // the comment and not about the fixture failing to carry debug info.
    let (code_start, code_end) = line_range_containing(&src, "~ temp after = 2");
    assert!(
        program
            .resolve_source_range("main.ink", code_start, code_end)
            .is_some(),
        "control: the adjacent executable line must bind, or this fixture \
         proves nothing about the comment"
    );
}

#[test]
fn an_empty_span_binds_to_nothing() {
    let (program, _) = compile_with_debug_info(&[("main.ink", "~ 1\n-> END\n")], "main.ink");
    assert_eq!(program.resolve_source_range("main.ink", 0, 0), None);
}

// ── Determinism: the earliest construct wins, not an arbitrary match ─────

#[test]
fn a_span_with_several_candidates_picks_the_textually_earliest() {
    // A whole-file span sweeps up every entry in every container. The
    // documented rule is minimum by (range_start, container_idx,
    // bytecode_offset) — so the answer must be the FIRST construct in the
    // file, and must be stable across runs.
    let src = "VAR x = 0\n~ x = 5\n~ x = 6\n~ x = 7\n-> END\n";
    let (program, src) = compile_with_debug_info(&[("main.ink", src)], "main.ink");
    let whole_file = u32::try_from(src.len()).expect("fits u32");

    let sweep = program
        .resolve_source_range("main.ink", 0, whole_file)
        .expect("a whole-file span must bind to something");
    let first_line = program
        .resolve_source_range("main.ink", 0, {
            let (_, end) = line_range_containing(&src, "~ x = 5");
            end
        })
        .expect("the leading span must bind");

    assert_eq!(
        sweep, first_line,
        "a span covering the whole file must resolve to the same address as \
         a span covering only its first executable construct — otherwise the \
         'textually earliest wins' rule is not what the code does"
    );

    // Stable across repeated calls: an implementation iterating a HashMap
    // could pass once and fail on the next run.
    for _ in 0..8 {
        assert_eq!(
            program.resolve_source_range("main.ink", 0, whole_file),
            Some(sweep),
            "repeated calls must agree — the tie-break has to be total"
        );
    }
}

// ── Gating: absent debug info, unknown file ─────────────────────────────

#[test]
fn without_debug_info_every_span_binds_to_nothing() {
    let out = brink_compiler::compile("main.ink", |_p| Ok("~ 1\n-> END\n".to_owned()))
        .expect("test source compiles");
    let (program, _) = brink_runtime::link(&out.data).expect("link");
    assert_eq!(
        program.resolve_source_range("main.ink", 0, 100),
        None,
        "no DebugInfo section means no answer — not a panic"
    );
}

#[test]
fn an_unknown_file_binds_to_nothing() {
    let (program, _) = compile_with_debug_info(&[("main.ink", "~ 1\n-> END\n")], "main.ink");
    assert_eq!(program.resolve_source_range("nope.ink", 0, 100), None);
}
