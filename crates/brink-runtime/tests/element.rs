//! Regression tests for [`OutputLine::element`] (`Element`, issue #1683) —
//! the per-line classification field the `Step`/`OutputLine` redesign
//! (#1684) reserved but never populated.
//!
//! Scoped narrowly and honestly (see `Element`'s own doc): a plain, ink-
//! dialect line with no `@[element]`/`@[convention]` dispatch still reports
//! the degenerate [`Element::narrative`] case — these tests pin that the
//! field exists, is always the narrative default there, and survives both a
//! plain line and a line inside a choice-driven run.
//!
//! Issue #2108 (`docs/decision-log.md` 2026-08-03 "The element output
//! model") populates `element.data` for the one case the field's own doc
//! now scopes as real: an `attach = StructName` convention handler's
//! claimed line consumes itself (no event) and merges its declared struct's
//! fields into the block-level state every line in the following run
//! carries a copy of. `attach_convention_data_reaches_the_following_run`
//! below is that proof, run against native (`.brink`) source — the only
//! dialect `@[convention]` dispatch exists in.

use brink_runtime::{Element, FastRng, Step, Story};

/// Compile ink source and link it into a runnable story.
#[expect(clippy::unwrap_used)]
fn story_from_source(src: &str) -> Story<FastRng> {
    let data = brink_compiler::compile("main.ink", |_p| Ok(src.to_owned()))
        .unwrap()
        .data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    Story::new(std::sync::Arc::new(program), line_tables)
}

/// Compile native (`.brink`) source — the `.brink` extension is what routes
/// `brink_compiler::compile_path` through the native dialect
/// (`brink_syntax_native` + `hir::lower_native`), the only frontend
/// `@[convention]`/attach-mode dispatch exists in today.
///
/// Native compilation goes through `brink_compiler::compile_path` rather
/// than the closure-based `compile` used by [`story_from_source`] above:
/// native source discovery is tree-is-universe (every `.brink` file under
/// the entry's root joins the project, `CLAUDE.md`'s own note on this), so
/// it needs a real file on a real filesystem to discover from — a bare
/// per-path read callback with no directory to scan fails with "entry file
/// not found after discovery." The entry lives alone in its own uniquely-
/// named temp subdirectory (never a shared one) — "several probe files in
/// one directory silently become one project" is a known hazard of this
/// same tree-is-universe discovery, so isolation is per-directory, not
/// merely per-filename.
#[expect(clippy::unwrap_used)]
fn story_from_native_source(src: &str) -> Story<FastRng> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("brink_element_test_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("main.brink");
    std::fs::write(&path, src).unwrap();
    // Issue #2289 part 2 (2026-08-05 ruling): a declared `@[convention]`
    // handler with no configured conventions module is now `E169`, not a
    // silent pass — `main.brink` inlines its own handlers, so it is its own
    // conventions module. `compile_path` never reads a co-located
    // `brink.toml` (its own doc says it bypasses `Environment`/config
    // entirely), so this has to be an explicit option, not a written file.
    let options = brink_analyzer::AnalysisOptions {
        conventions: Some("main.brink".to_owned()),
        ..brink_analyzer::AnalysisOptions::default()
    };
    let result = brink_compiler::compile_path_with_options(&path, options);
    let _ = std::fs::remove_dir_all(&dir);
    let data = result.unwrap().data;
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    Story::new(std::sync::Arc::new(program), line_tables)
}

/// Every `Step::Line` carries the degenerate narrative element — no
/// `@[element]` dispatch exists in this fixture, so there is nothing to
/// classify beyond the always-correct default.
#[test]
fn plain_lines_carry_the_narrative_default() {
    let mut story = story_from_source("One.\nTwo.\n-> END\n");
    let steps = story.continue_maximally().expect("drive to END");

    let lines: Vec<_> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Line(line) => Some(line),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 2, "{steps:?}");
    for line in &lines {
        assert_eq!(line.element, Element::narrative(), "{steps:?}");
        assert_eq!(line.element.kind, "narrative");
        assert!(line.element.data.is_empty());
    }
}

/// The narrative default survives a choice-driven branch too — `element`
/// is stamped independently of `block_id`, so a fresh run after a choice
/// still reports the same degenerate classification.
#[test]
fn lines_after_a_choice_still_carry_the_narrative_default() {
    let mut story = story_from_source(
        "-> start\n\
         === start ===\n\
         Before.\n\
         * [left] Went left.\n-> END\n\
         * [right] Went right.\n-> END\n",
    );
    let _ = story.continue_maximally().expect("drive to choices");
    story.choose(0).expect("choose left");
    let after_steps = story.continue_maximally().expect("drive to END");

    let after_lines: Vec<_> = after_steps
        .iter()
        .filter_map(|s| match s {
            Step::Line(line) => Some(line),
            _ => None,
        })
        .collect();
    assert!(!after_lines.is_empty(), "{after_steps:?}");
    for line in &after_lines {
        assert_eq!(line.element, Element::narrative(), "{after_steps:?}");
    }
}

/// Issue #2108, the actual payoff: dialogue lines report their speaker.
///
/// Mirrors `tests/tier1-native/conventions-screenplay-preset/story.brink`'s
/// shape (two speaker turns, the second with no `parenthetical`) closely
/// enough to pin every corner the ruled model names:
///
/// - Ruling item 6 ("AN EVENT EXISTS IFF A LINE EXISTS"): `cue`/
///   `parenthetical`'s own claimed lines (`@VENDOR`, `(hushed)`, `@KID`)
///   produce **no** `Step::Line` at all — only the three ordinary dialogue/
///   transition lines do.
/// - Ruling item 3 ("attachment ACCUMULATES onto the following run"):
///   `cue` then `parenthetical` both merge into the SAME dialogue line's
///   data — `{speaker: VENDOR, delivery: hushed}`, not just one or the
///   other.
/// - Ruling item 4/`ElementAttachment`'s own doc ("the run IS the block"):
///   `KID`'s cue does not inherit `VENDOR`'s already-consumed data, and the
///   bare `transition` line after it carries NO attachment at all — the
///   block-level state resets between runs rather than leaking forward
///   forever.
#[test]
fn attach_convention_data_reaches_the_following_run() {
    let src = r#"
struct Cue {
  speaker: string,
}

struct Parenthetical {
  delivery: string,
}

@[convention(claims = "^(?<name>[A-Z][A-Z '-]*)$", attach = Cue, order = 10)]
fn cue(name: string): Cue {
  return Cue { speaker: name };
}

@[convention(claims = "^(?<delivery>[a-z][a-z' -]*)$", attach = Parenthetical, order = 20)]
fn parenthetical(delivery: string): Parenthetical {
  return Parenthetical { delivery: delivery };
}

@[convention(claims = "^(?<text>[A-Z][A-Z '-]*:)$", order = 30)]
fn transition(text: string) {
  return text;
}

flow main() {
  @VENDOR
  (hushed)
  You shouldn't be here after dark.

  @KID
  Says who?

  CUT TO:
  -> END
}
"#;
    let mut story = story_from_native_source(src);
    let steps = story.continue_maximally().expect("drive to END");

    let lines: Vec<_> = steps
        .iter()
        .filter_map(|s| match s {
            Step::Line(line) => Some(line),
            _ => None,
        })
        .collect();

    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "You shouldn't be here after dark.\n",
            "Says who?\n",
            "CUT TO:\n",
        ],
        "attach conventions must consume their own line and produce no event: {steps:?}"
    );

    let vendor_line = lines[0];
    assert_eq!(
        vendor_line.element.data.get("speaker").map(String::as_str),
        Some("VENDOR"),
        "{vendor_line:?}"
    );
    assert_eq!(
        vendor_line.element.data.get("delivery").map(String::as_str),
        Some("hushed"),
        "{vendor_line:?}"
    );

    let kid_line = lines[1];
    assert_eq!(
        kid_line.element.data.get("speaker").map(String::as_str),
        Some("KID"),
        "{kid_line:?}"
    );
    assert!(
        !kid_line.element.data.contains_key("delivery"),
        "KID's turn has no parenthetical — must not inherit VENDOR's 'hushed': {kid_line:?}"
    );

    let transition_line = lines[2];
    assert_eq!(
        transition_line.element,
        Element::narrative(),
        "a bare transition after a dialogue run must not inherit its speaker: {transition_line:?}"
    );
}

/// Review finding on issue #2108's PR: `OutputBuffer::flush_lines` seeded
/// `resolve_lines_annotated` with `pending_element` but never wrote the
/// end-of-slice state back (unlike `take_first_line`, which does) — so an
/// `ElementAttachEnd` consumed by a yield-point flush (a choice boundary,
/// here) was lost and the attach data stayed live on every line
/// afterward, in a different block.
///
/// `@VENDOR`'s two-line run is followed immediately by a choice point: the
/// trailing dialogue line has nothing after it in the transcript to prove
/// its own completion via `take_first_line` before the story yields
/// `Step::Choices`, so it — and the run-closing `ElementAttachEnd` — drain
/// through `flush_lines` at that yield point instead. Once a choice is
/// taken, the chosen branch's own line(s) belong to a new block entirely
/// and must not inherit `VENDOR`'s speaker.
#[test]
fn attach_element_data_does_not_leak_across_a_choice_boundary() {
    let src = r#"
struct Cue {
  speaker: string,
}

@[convention(claims = "^(?<name>[A-Z][A-Z '-]*)$", attach = Cue, order = 10)]
fn cue(name: string): Cue {
  return Cue { speaker: name };
}

flow main() {
  @VENDOR
  You shouldn't be here after dark.
  Get out now.

  {?
    * [Leave] You leave without a word.
  }
  -> END
}
"#;
    let mut story = story_from_native_source(src);
    let before_steps = story.continue_maximally().expect("drive to choices");
    assert!(
        matches!(before_steps.last(), Some(Step::Choices(_))),
        "{before_steps:?}"
    );

    story.choose(0).expect("choose Leave");
    let after_steps = story.continue_maximally().expect("drive to END");

    let after_lines: Vec<_> = after_steps
        .iter()
        .filter_map(|s| match s {
            Step::Line(line) => Some(line),
            _ => None,
        })
        .collect();
    assert!(!after_lines.is_empty(), "{after_steps:?}");
    for line in &after_lines {
        assert_eq!(
            line.element,
            Element::narrative(),
            "a line in the branch taken after the choice must not inherit \
             VENDOR's already-closed attach run: {after_steps:?}"
        );
    }
}
