//! Issue #3538: a shuffle's partial Fisher-Yates removed the picked index
//! with `Vec::swap_remove`, which moves the last unpicked element into the
//! hole. The reference removes it order-preservingly
//! (`unpickedIndices.RemoveAt` / `splice(chosen, 1)`), so from the second
//! draw of a loop onward brink indexed a differently-ordered list and
//! picked a different element — while the first draw of each loop still
//! agreed, since nothing has been removed yet.
//!
//! That signature is what the corpus showed before the fix
//! (`tier2/conditional/shuffle`): within one loop of three, iteration 0
//! matched and iterations 1 and 2 were swapped with each other.
//!
//! Reference output below is inkjs 2.4.0 via `tools/inkjs-oracle` (the
//! sanctioned stand-in, `docs/program-generator-spec.md` §6); the same
//! shape is covered against a C# golden by `tier2/conditional/shuffle` and
//! `tier2/sequences/I107-shuffle-stack-muddying`, both of which flip from
//! failing to passing in this change.

// Integration-test convention across this directory: helpers outside
// `#[test]` fns are not covered by clippy.toml's test carve-out.
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use brink_runtime::{DotNetRng, Step, Story};

/// Compile one in-memory file, play it, and take choice 0 whenever one is
/// offered, until `limit` non-blank lines have been delivered.
fn play_looping(source: &str, limit: usize) -> Vec<String> {
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
    for _ in 0..400 {
        match story.continue_single().expect("runtime") {
            Step::Line(l) => {
                let text = l.text.trim().to_owned();
                if !text.is_empty() {
                    lines.push(text);
                    if lines.len() == limit {
                        return lines;
                    }
                }
            }
            Step::Choices(_) => story.choose(0).expect("choose 0"),
            Step::Done => {}
            Step::End | Step::Suspended => return lines,
        }
    }
    panic!("story did not deliver {limit} lines in 400 steps");
}

/// Four loops of a three-way shuffle, so every `iterationIndex` (0, 1, 2) is
/// exercised repeatedly. inkjs draws
/// `alpha bravo charlie | alpha charlie bravo | bravo alpha charlie |
/// bravo alpha charlie`.
///
/// Under `swap_remove` the first draw of each loop still matched — nothing
/// has been removed yet — and the other two came out permuted, so it is the
/// second and third of each loop that separate the two removals. A test
/// drawing once per loop would pass either way.
///
/// The shuffle sits in a knot with a choice deliberately: that is the shape
/// whose container path brink and inklecate already agree on (`k.0.0`), so
/// this test isolates the removal order. The *other* shuffle divergence
/// (#3538 proper — brink inserts an implicit stitch level inklecate does
/// not, making a choice-free knot's sequence `k.0.0` against ink's `k.0`)
/// would otherwise confound it.
#[test]
fn shuffle_removes_the_picked_index_order_preservingly() {
    let src = concat!(
        "-> k\n",
        "\n",
        "=== k ===\n",
        "{ shuffle:\n",
        "    - alpha\n",
        "    - bravo\n",
        "    - charlie\n",
        "}\n",
        "+ [again] -> k\n",
    );
    assert_eq!(
        play_looping(src, 12),
        [
            "alpha", "bravo", "charlie", "alpha", "charlie", "bravo", "bravo", "alpha", "charlie",
            "bravo", "alpha", "charlie"
        ]
    );
}
