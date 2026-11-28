//! Issue #3528: a temp written after `<- thread` was missing at the next
//! choice point. `CallStack` caches its last snapshot for cheap forks;
//! `<-` forks the main thread (caching its frames), the later `~ temp`
//! wrote into the top frame through `last_mut` without invalidating the
//! cache, and the choice's fork was served the stale frames — so choosing
//! restored a stack without the temp. Reference outputs below are inkjs
//! 2.4.0 via `tools/inkjs-oracle` (the sanctioned stand-in,
//! `docs/program-generator-spec.md` §6); the corpus case with a C# golden
//! is owed (dotnet, maintainer-local).

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file and play it, choosing `choices` in order,
/// returning every delivered line's text verbatim.
fn play_choosing(source: &str, choices: &[usize]) -> Vec<String> {
    let output = brink_compiler::compile("story.ink", |_| Ok(source.to_owned()));
    assert!(
        output.is_ok(),
        "compile failed: {:?}\n{source}",
        output.as_ref().err()
    );
    let output = output.expect("just asserted above");
    let (program, line_tables) = brink_runtime::link(&output.data).expect("link");
    let mut story = Story::<DotNetRng>::new(Arc::new(program), line_tables);
    let mut lines = Vec::new();
    let mut picks = choices.iter().copied();
    for _ in 0..200 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => lines.push(l.text.clone()),
            Step::Choices(_) => {
                let pick = picks
                    .next()
                    .unwrap_or_else(|| panic!("unexpected choices in {source}"));
                story.choose(pick).expect("choose");
            }
            Step::Done | Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not reach a terminal step in 200 steps");
}

/// The shrunk differential counterexample (the choice's own text is
/// bracketed, so nothing is echoed). Reference: `1`.
#[test]
fn temp_written_after_a_thread_spawn_survives_the_choice() {
    let src = "-> k\n\n=== k ===\n<- t\n~ temp t0 = 1\n* [a]\n+ -> END\n- {t0}\n-> END\n\n=== t ===\n-> DONE\n";
    assert_eq!(play_choosing(src, &[0]), vec!["1\n"]);
}

/// A thread that prints, then the same shape. Reference: `thread text`,
/// `1`.
#[test]
fn temp_after_a_printing_thread_survives_the_choice() {
    let src = "-> k\n\n=== k ===\n<- t\n~ temp t0 = 1\n* [a]\n+ -> END\n- {t0}\n-> END\n\n=== t ===\nthread text\n-> DONE\n";
    assert_eq!(play_choosing(src, &[0]), vec!["thread text\n", "1\n"]);
}

/// A temp declared before the spawn was already right; unchanged.
/// Reference: `1`.
#[test]
fn temp_written_before_the_spawn_control() {
    let src = "-> k\n\n=== k ===\n~ temp t0 = 1\n<- t\n* [a]\n+ -> END\n- {t0}\n-> END\n\n=== t ===\n-> DONE\n";
    assert_eq!(play_choosing(src, &[0]), vec!["1\n"]);
}

/// A temp re-assigned after the spawn: the choice sees the new value.
/// Reference: `2`.
#[test]
fn temp_reassigned_after_the_spawn_survives_the_choice() {
    let src = "-> k\n\n=== k ===\n~ temp t0 = 1\n<- t\n~ t0 = 2\n* [a]\n+ -> END\n- {t0}\n-> END\n\n=== t ===\n-> DONE\n";
    assert_eq!(play_choosing(src, &[0]), vec!["2\n"]);
}
