#![no_main]

use std::sync::Arc;

use brink_format::read_inkb;
use brink_runtime::{Line, Story, link};
use libfuzzer_sys::fuzz_target;

/// Bound on story turns driven per fuzz input, independent of the VM's own
/// per-`continue_maximally` step budget (`Story::STEP_LIMIT`, which this
/// target never raises — a malformed program that runs away is expected to
/// hit `RuntimeError::StepLimitExceeded` and stop). This second bound exists
/// because a crafted program can legitimately re-yield `Choices` many times
/// without ever tripping the VM step limit (each turn is cheap on its own);
/// without a cap here, a single fuzz iteration could still take unbounded
/// wall-clock time to reach a terminal state.
const MAX_TURNS: usize = 2_000;

// Feed arbitrary/mutated bytes as a `.inkb` program: decode, link, and drive
// it through the real `Story` execution API exactly as any consumer would.
// The VM's step-limit hardening (`RuntimeError::StepLimitExceeded`) and the
// linker's own validation are the only sanctioned way for a malformed
// program to stop — everything else must be a clean `Err`, never a panic
// or an unbounded loop.
fuzz_target!(|data: &[u8]| {
    let Ok(story_data) = read_inkb(data) else {
        return;
    };
    let Ok((program, line_tables)) = link(&story_data) else {
        return;
    };

    let program = Arc::new(program);
    let mut story: Story = Story::new(program, line_tables);

    for _ in 0..MAX_TURNS {
        let lines = match story.continue_maximally() {
            Ok(lines) => lines,
            // A clean runtime fault (including StepLimitExceeded) is a
            // legitimate, expected outcome for malformed bytecode — stop.
            Err(_) => break,
        };

        match lines.last() {
            Some(Line::Choices { choices, .. }) => {
                if choices.is_empty() || story.choose(0).is_err() {
                    break;
                }
            }
            // `Done` (ink `-> DONE`) ends this turn's output but not the
            // story — the docs on `Line::Done` say to call
            // `continue_maximally` again, so loop around and do that.
            Some(Line::Done { .. }) => {}
            // `End` (permanent), `Text` (shouldn't be last per
            // `is_terminal`), or an empty batch — nothing more to drive.
            _ => break,
        }
    }
});
