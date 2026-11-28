//! Shared-context flows (#200): a named flow that shares `default_context`
//! (globals / visit counts / rng) with the default flow, while keeping its own
//! call stack. Drives the studio's "+ new flow" feature.

#![expect(clippy::unwrap_used, clippy::panic)]

use brink_runtime::{DotNetRng, Step, Story};

fn story_from(case: &str) -> (brink_runtime::Program, Vec<Vec<brink_format::LineEntry>>) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/tier1")
        .join(case)
        .join("story.ink");
    let data = brink_compiler::compile_path(&path).unwrap().data;
    brink_runtime::link(&data).unwrap()
}

/// Run a shared flow to a terminal line.
fn run_flow(story: &mut Story<DotNetRng>, name: &str) {
    for _ in 0..1000 {
        match story.continue_flow_single(name).unwrap() {
            Step::Line(_) => {}
            Step::Done | Step::End | Step::Choices(_) | Step::Suspended => {
                return;
            }
        }
    }
    panic!("flow did not terminate");
}

#[test]
fn shared_flow_writes_are_visible_in_the_default_context() {
    let (program, line_tables) = story_from("knots/knot-stitch-gather-counts");
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    // Spawn a shared flow at the root and run it — WITHOUT advancing the
    // default flow at all.
    story.spawn_flow_shared("b", None).unwrap();
    run_flow(&mut story, "b");

    // The default flow never ran, yet its context records the shared flow's
    // visits — proving the context (visit counts) is shared (#200).
    let default_snapshot = story.debug_snapshot();
    assert!(
        !default_snapshot.visit_counts.is_empty(),
        "shared flow's visits must appear in the default (shared) context",
    );

    // The two flows are nonetheless distinct: the shared flow has reached a
    // terminal status while the default flow is still untouched.
    let flow_snapshot = story.debug_snapshot_flow("b").unwrap();
    assert_ne!(
        flow_snapshot.status, "active",
        "the shared flow ran to a terminal status",
    );
    assert_eq!(
        default_snapshot.status, "active",
        "the default flow was never advanced",
    );
}

#[test]
fn shared_flows_are_listed_and_destroyable() {
    let (program, line_tables) = story_from("knots/knot-stitch-gather-counts");
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    story.spawn_flow_shared("alpha", None).unwrap();
    story.spawn_flow_shared("beta", None).unwrap();
    assert_eq!(story.flow_names(), vec!["alpha", "beta"]); // sorted, deterministic

    // Re-spawning the same name errors.
    assert!(story.spawn_flow_shared("alpha", None).is_err());

    story.destroy_flow("alpha").unwrap();
    assert_eq!(story.flow_names(), vec!["beta"]);
    assert!(story.debug_snapshot_flow("alpha").is_err()); // gone
    assert!(story.destroy_flow("alpha").is_err()); // already gone
}
