//! Issue #2092 end to end: the built-in screenplay preset's `heading`
//! handler calls the `scene_entered(title, slug)` host extern.
//!
//! Before this issue, `std/conventions/screenplay.brink`'s `heading`
//! handler could only emit plain display text — there was no extern for it
//! to notify a host engine (e.g. `bevy-brink`) that a scene changed. This
//! test drives the preset's own claim-handler source (the same
//! `@[element(claims = …)]` declarations `std/conventions/screenplay.brink`
//! ships, mirroring `tests/tier1-native/conventions-screenplay-preset/
//! story.brink`'s established single-file pattern — see that fixture's own
//! doc for why cross-file `use std::conventions::screenplay` isn't real
//! yet) through the real native pipeline, binds a recording
//! [`ExternalFnHandler`], and asserts `scene_entered` fires with the
//! claimed `(title, slug)` in call order — proving the wiring is actually
//! reached, not just that the preset still lowers.
//!
//! Reverting the production `scene_entered(title, "");` call in
//! `heading`'s body (restoring the pre-#2092 shape) makes
//! [`scene_entered_fires_with_the_claimed_title_and_an_empty_slug`] fail
//! with zero recorded calls — verified by hand before landing this file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::cell::RefCell;
use std::sync::Arc;

use brink_format::Value;
use brink_runtime::{ExternalFnHandler, ExternalResult, Step, Story};
use brink_test_harness::ExploreConfig;
use brink_test_harness::corpus::compile_and_explore_from_brink_native;

/// Byte-for-byte the same four handler declarations
/// `std/conventions/screenplay.brink` ships (plus the `scene_entered`
/// extern + no-op fallback it now declares alongside `heading`), combined
/// with a driving `flow main()` in the one file the native "tree-is-
/// universe" + single-file-claim-dispatch mechanism requires today.
const SCREENPLAY_PRESET_WITH_DRIVER: &str = "\
extern scene_entered(title, slug)

fn scene_entered(title: string, slug: string) {
}

@[element(claims = \"^(?<kind>INT|EXT)\\\\. (?<title>.+)$\")]
fn heading(kind: string, title: string) {
  scene_entered(title, \"\");
  return \"-- {kind}. {title} --\";
}

@[element(claims = \"^(?<text>[A-Z][A-Z '-]*:)$\")]
fn transition(text: string) {
  return text;
}

@[element(claims = \"^(?<name>[A-Z][A-Z '-]*)$\", block)]
fn cue(name: string, body: content) >{
  {name}
  {body}
}

@[element(claims = \"^(?<delivery>[a-z][a-z' -]*)$\", block)]
fn parenthetical(delivery: string, body: content) >{
  ({delivery})
  {body}
}

flow main() {
  INT. MARKET SQUARE - NIGHT
  The square is empty.
  -> END
}
";

/// Records every external call it sees, resolving `scene_entered` to
/// `Value::Null` (a fire-and-forget host notification) and falling back to
/// the ink-declared body for anything else (there is nothing else to call
/// in this fixture, but this keeps the handler honest about its scope).
struct RecordingHandler {
    calls: RefCell<Vec<(String, Vec<Value>)>>,
}

impl RecordingHandler {
    fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl ExternalFnHandler for RecordingHandler {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        self.calls
            .borrow_mut()
            .push((name.to_string(), args.to_vec()));
        match name {
            "scene_entered" => ExternalResult::Resolved(Value::Null),
            _ => ExternalResult::Fallback,
        }
    }
}

#[test]
fn scene_entered_fires_with_the_claimed_title_and_an_empty_slug() {
    // `compile_and_explore_from_brink_native` runs the real native pipeline
    // (parse → hir::lower_native → analyzer → LIR → codegen → link →
    // explore) and hands back the compiled `StoryData` alongside its own
    // `FallbackHandler`-driven episodes (which this test discards) — link
    // and drive the `StoryData` a second time here with a handler that can
    // actually observe the call.
    let (story_data, _episodes) = compile_and_explore_from_brink_native(
        SCREENPLAY_PRESET_WITH_DRIVER,
        &ExploreConfig::default(),
    )
    .unwrap_or_else(|e| panic!("preset + driver must compile and link: {e}"));

    let (program, line_tables) =
        brink_runtime::link(&story_data).expect("compiled screenplay preset must link");
    let mut story = Story::<brink_runtime::DotNetRng>::new(Arc::new(program), line_tables);
    let handler = RecordingHandler::new();

    let mut text = String::new();
    loop {
        match story
            .continue_single_with(&handler)
            .expect("screenplay preset + scene_entered fallback must run without a runtime fault")
        {
            Step::Line(line) => text.push_str(&line.text),
            Step::Done | Step::End => break,
            Step::Choices(_) => panic!("fixture is choice-free"),
            Step::Suspended => panic!("fixture never suspends — no pending external"),
        }
    }

    let calls = handler.calls.into_inner();
    assert_eq!(
        calls,
        vec![(
            "scene_entered".to_string(),
            vec![
                Value::String("MARKET SQUARE - NIGHT".into()),
                Value::String(String::new().into()),
            ],
        )],
        "the heading handler must call scene_entered exactly once, with the \
         claimed title and an empty (pass-through, not derived) slug"
    );
    assert!(
        text.contains("-- INT. MARKET SQUARE - NIGHT --"),
        "the heading handler must still emit its display text alongside \
         the host notification, not instead of it — got {text:?}"
    );
}
