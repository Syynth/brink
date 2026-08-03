//! NS-A6 rng-as-cell integration tests (issue #1112,
//! `docs/stdlib-spec.md` §7; `docs/stdlib-sequencing.md` §2 Wave A6).
//!
//! The wave's gate, end-to-end through the real compiler + runtime:
//!
//! - **Seeded replay = identical transcript**: same seed, fresh stories,
//!   byte-identical output — multiple runs, both built-in generators.
//! - **One cell, two surfaces**: ink's frozen `SEED_RANDOM`/`RANDOM` and
//!   the brink draw verbs read and advance the SAME `(rng_seed,
//!   previous_random)` state — interchanging the seeding spelling changes
//!   nothing.
//! - **State saves/loads with the story**: the RNG cell round-trips
//!   through `SaveState` (including a serde wire trip), and restoring a
//!   mid-story save replays the exact draw sequence from the save point —
//!   not merely "the story is deterministic from the top".

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use brink_compiler::{AnalysisOptions, Dialect};
use brink_runtime::{DotNetRng, FastRng, Step, Story, StoryRng};
use std::sync::Arc;

fn compile_and_link(
    source: &str,
) -> (
    std::sync::Arc<brink_runtime::Program>,
    Vec<Vec<brink_format::LineEntry>>,
) {
    let files: std::collections::HashMap<&str, &str> =
        std::collections::HashMap::from([("main.ink", source)]);
    let output = brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files
                .get(path)
                .map(|s| (*s).to_string())
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, path))
        },
        AnalysisOptions {
            dialect: Dialect::Brink,
            ..AnalysisOptions::default()
        },
    )
    .expect("compile");
    let (program, tables) = brink_runtime::link(&output.data).expect("link");
    (std::sync::Arc::new(program), tables)
}

/// Run to the next terminal (`Done`/`End`) or choice point, appending text.
/// Returns `true` when stopped at a choice point.
fn run_segment<R: StoryRng>(story: &mut Story<R>, out: &mut String) -> bool {
    loop {
        match story.continue_single().expect("runtime error") {
            Step::Line(line) => out.push_str(&line.text),
            Step::Choices(_) => return true,
            Step::Done | Step::End | Step::Suspended => return false,
        }
    }
}

fn run_to_end<R: StoryRng>(story: &mut Story<R>) -> String {
    let mut out = String::new();
    let stopped_at_choice = run_segment(story, &mut out);
    assert!(!stopped_at_choice, "straight-line story hit a choice");
    out
}

/// A draw-heavy straight-line story: every verb, plus the frozen ink
/// `RANDOM`, all chained off one `seed(n)` — enough draws that two seeds
/// colliding by accident is out of the question.
fn draw_heavy_story(seed_stmt: &str) -> String {
    format!(
        "{seed_stmt}\n\
         ~ temp a = #[10, 20, 30, 40, 50]\n\
         draws: {{draws(10)}}\n\
         picked {{string(pick(a))}}\n\
         ~ shuffle(a)\n\
         after shuffle: {{a[0]}} {{a[1]}} {{a[2]}} {{a[3]}} {{a[4]}}\n\
         functional: {{len(shuffled(a))}}\n\
         -> END\n\
         \n\
         === function draws(n) ===\n\
         ~ temp acc = \"\"\n\
         ~ temp i = 0\n\
         ~ {{\n\
         while i < n {{\n\
         acc = acc + \" \" + string(float()) + \":\" + string(chance(0.5)) + \":\" + string(RANDOM(1, 100))\n\
         i = i + 1\n\
         }}\n\
         }}\n\
         ~ return acc\n"
    )
}

#[test]
fn seeded_replay_is_deterministic_across_runs() {
    let src = draw_heavy_story("~ seed(1234)");
    let (program, tables) = compile_and_link(&src);
    let mut transcripts = Vec::new();
    for _ in 0..3 {
        let mut story = Story::<DotNetRng>::new(Arc::clone(&program), tables.clone());
        transcripts.push(run_to_end(&mut story));
    }
    assert_eq!(transcripts[0], transcripts[1], "same seed, same transcript");
    assert_eq!(transcripts[1], transcripts[2], "every run identical");
    assert!(
        transcripts[0].contains("after shuffle"),
        "the story actually ran: {}",
        transcripts[0]
    );

    // A different seed produces a different draw stream (40+ draws — a
    // collision would mean the seed isn't reaching the cell).
    let other_src = draw_heavy_story("~ seed(99)");
    let (program2, tables2) = compile_and_link(&other_src);
    let mut story = Story::<DotNetRng>::new(program2, tables2);
    let other = run_to_end(&mut story);
    assert_ne!(transcripts[0], other, "distinct seeds must diverge");
}

#[test]
fn seeded_replay_is_deterministic_for_fast_rng_too() {
    // The determinism contract is generator-generic: the pinned chain is
    // pure state → (value, state') for ANY `StoryRng`.
    let src = draw_heavy_story("~ seed(777)");
    let (program, tables) = compile_and_link(&src);
    let mut a = Story::<FastRng>::new(Arc::clone(&program), tables.clone());
    let mut b = Story::<FastRng>::new(program, tables);
    assert_eq!(run_to_end(&mut a), run_to_end(&mut b));
}

#[test]
fn host_seeding_via_set_rng_seed_is_deterministic() {
    // The unseeded-story posture: seeding is a host concern at story
    // start — `Story::set_rng_seed` before the first continue plays the
    // role `seed(n)` plays in-story.
    let src = draw_heavy_story("// host-seeded");
    let (program, tables) = compile_and_link(&src);
    let run = |seed: i32| {
        let mut story = Story::<DotNetRng>::new(Arc::clone(&program), tables.clone());
        story.set_rng_seed(seed);
        run_to_end(&mut story)
    };
    assert_eq!(run(5), run(5), "host-seeded replay is identical");
    assert_ne!(run(5), run(6), "host seeds reach the cell");
}

#[test]
fn ink_and_brink_seeding_spellings_share_the_one_cell() {
    // `seed(n)` lowers to the frozen `SeedRandom` op — swapping the
    // spelling changes nothing about any subsequent draw on either
    // surface. This is the "one cell, two surfaces, no drift" gate.
    let brink_src = draw_heavy_story("~ seed(2026)");
    let ink_src = draw_heavy_story("~ SEED_RANDOM(2026)");
    let (pa, ta) = compile_and_link(&brink_src);
    let (pb, tb) = compile_and_link(&ink_src);
    let mut a = Story::<DotNetRng>::new(pa, ta);
    let mut b = Story::<DotNetRng>::new(pb, tb);
    assert_eq!(
        run_to_end(&mut a),
        run_to_end(&mut b),
        "seed(n) and SEED_RANDOM(n) must be the same cell write"
    );
}

/// The save/load gate, designed so it can only pass if the CELL restores —
/// not merely because the story is deterministic from the top: run B
/// consumes a DIFFERENT number of draws than run A before the save state
/// is loaded, so only a genuine cell restore aligns the tails.
#[test]
fn save_load_round_trips_the_rng_cell_mid_story() {
    let src = "~ seed(31337)\n\
               -> turn\n\
               === turn ===\n\
               draw {float()} roll {RANDOM(1, 1000)}\n\
               + [again] -> turn\n\
               + [stop] -> END\n";
    let (program, tables) = compile_and_link(src);

    // Run A: one turn (2 draws), save, then a second turn — the tail.
    let mut a = Story::<DotNetRng>::new(Arc::clone(&program), tables.clone());
    let mut head_a = String::new();
    assert!(run_segment(&mut a, &mut head_a), "expected a choice point");
    let save = a.save_state();

    // Serde wire trip: the cell fields must survive serialization.
    let json = serde_json::to_string(&save).expect("serialize");
    let restored: brink_format::SaveState = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        restored.rng_seed, save.rng_seed,
        "rng_seed survives the wire"
    );
    assert_eq!(
        restored.previous_random, save.previous_random,
        "previous_random survives the wire"
    );

    a.choose(0).expect("choose again");
    let mut tail_a = String::new();
    assert!(run_segment(&mut a, &mut tail_a), "still at a choice");

    // Run B: TWO turns (4 draws — cell state now differs from the save
    // point), then load the wire-tripped save and take one more turn.
    let mut b = Story::<DotNetRng>::new(program, tables);
    let mut scratch = String::new();
    assert!(run_segment(&mut b, &mut scratch));
    b.choose(0).expect("choose again");
    assert!(run_segment(&mut b, &mut scratch));
    let report = b.load_state(&restored);
    assert!(
        report.unknown_globals.is_empty(),
        "clean load, got {report:?}"
    );
    b.choose(0).expect("choose again");
    let mut tail_b = String::new();
    assert!(run_segment(&mut b, &mut tail_b));

    assert_eq!(
        tail_a, tail_b,
        "after loading the save, the draw sequence must resume from the \
         saved cell state, not from wherever run B had advanced to"
    );
    assert_ne!(
        head_a, tail_a,
        "sanity: consecutive turns draw different values (the cell advances)"
    );
}
