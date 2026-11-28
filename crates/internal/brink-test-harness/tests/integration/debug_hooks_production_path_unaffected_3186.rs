//! Regression pin for the D8 (issue #3186) zero-cost claim: this exact
//! fixture's production `Stats` counters and output are asserted
//! byte/value-identical regardless of whether `brink-runtime`'s
//! `debug-hooks` feature is compiled in.
//!
//! Deliberately carries **no** `required-features` and uses **no**
//! `brink_runtime::debug_control`/`Story::debug_*` API — it compiles and
//! runs unconditionally. That is the point: `debug_control`'s own module
//! doc argues that nothing in `vm::step_impl`/`FlowInstance::advance_with_limit`
//! (the production per-turn loop this fixture exercises through the
//! ordinary `continue_maximally` path) changes when the feature is
//! toggled — since this file doesn't even know the feature exists, running
//! it once with the default feature set and once with
//! `--features debug-hooks` (as the D8 build gate does, both configurations
//! of the full workspace gate) exercises the identical assertions both
//! times. If the feature ever leaked so much as one extra branch into the
//! hot loop, that leak would have to change either the exact step/opcode
//! counters or the produced text below to be observed at all — and it
//! can't, because this file's source bytes are identical in both runs.

#![expect(clippy::expect_used, reason = "test helper: panic on bad fixtures")]

use std::sync::Arc;

use brink_runtime::{FastRng, Step, Story};

fn compile_ink(src: &str) -> (brink_runtime::Program, Vec<Vec<brink_format::LineEntry>>) {
    let out = brink_compiler::compile("t.ink", |p| {
        if p == "t.ink" {
            Ok(src.to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no such include",
            ))
        }
    })
    .expect("compile .ink");
    brink_runtime::link(&out.data).expect("link")
}

#[test]
fn production_step_count_and_output_are_unaffected_by_the_debug_hooks_feature() {
    let (program, tables) = compile_ink(
        "Hello.\n\
         World.\n\
         -> DONE\n",
    );
    let mut story = Story::<FastRng>::new(Arc::new(program), tables);

    let steps = story.continue_maximally().expect("continue_maximally");
    let text: String = steps
        .iter()
        .filter_map(|s| match s {
            Step::Line(l) => Some(l.text.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(text, "Hello.\nWorld.\n");
    assert!(
        matches!(steps.last(), Some(Step::Done)),
        "expected the run to end on Step::Done, got {:?}",
        steps.last()
    );

    let stats = story.stats();
    assert_eq!(
        stats.steps, 5,
        "production VM step count for this fixture must stay exactly 5 \
         regardless of the debug-hooks feature state — a change here \
         would mean debug_control leaked into the production hot loop"
    );
    assert_eq!(stats.frames_pushed, 0, "no calls in this fixture");
    assert_eq!(stats.threads_created, 0, "no threads in this fixture");
}
