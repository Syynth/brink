//! Branch exploration via DFS with Story cloning.
//!
//! Each step corresponds to one `continue_single_observed` call, matching
//! the oracle's per-`Continue()` granularity.

use brink_format::Value;
use brink_runtime::{DotNetRng, Line, Program, Story, WriteObserver};

use crate::episode::{
    ChoiceRecord, Episode, Outcome, StateSnapshot, StateWrite, StepOutcome, StepRecord,
};
use crate::termination::{classify_done, terminal_step};

/// Configuration for branch exploration.
pub struct ExploreConfig {
    /// Maximum recursion depth (choice selections).
    pub max_depth: usize,
    /// Maximum total episodes to collect.
    pub max_episodes: usize,
}

impl Default for ExploreConfig {
    fn default() -> Self {
        Self {
            max_depth: 20,
            max_episodes: 1_000,
        }
    }
}

/// Explore all reachable branches of a story via DFS.
///
/// Requires `Story: Clone` — each branch point clones the story state
/// and recurses. Returns one [`Episode`] per terminal path.
pub fn explore(
    program: std::sync::Arc<Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    config: &ExploreConfig,
) -> Vec<Episode> {
    let initial_state = StateSnapshot {
        globals: program.global_defaults(),
    };
    let story = Story::<DotNetRng>::new(program, line_tables);
    let mut episodes = Vec::new();

    explore_inner(
        story,
        config,
        &initial_state,
        &mut episodes,
        Vec::new(),
        Vec::new(),
        0,
    );

    episodes
}

/// Internal observer for exploration.
struct ExploreRecorder {
    writes: Vec<StateWrite>,
}

impl ExploreRecorder {
    fn new() -> Self {
        Self { writes: Vec::new() }
    }

    fn drain(&mut self) -> Vec<StateWrite> {
        core::mem::take(&mut self.writes)
    }
}

impl WriteObserver for ExploreRecorder {
    fn on_set_global(&mut self, idx: u32, value: &Value) {
        self.writes.push(StateWrite::SetGlobal {
            idx,
            value: value.clone(),
        });
    }

    fn on_increment_visit(&mut self, id: brink_format::DefinitionId, new_count: u32) {
        self.writes
            .push(StateWrite::IncrementVisit { id, new_count });
    }

    fn on_set_turn_count(&mut self, id: brink_format::DefinitionId, turn: u32) {
        self.writes.push(StateWrite::SetTurnCount { id, turn });
    }

    fn on_increment_turn_index(&mut self, new_value: u32) {
        self.writes
            .push(StateWrite::IncrementTurnIndex { new_value });
    }

    fn on_set_rng_seed(&mut self, new_seed: i32) {
        self.writes.push(StateWrite::SetRngSeed { new_seed });
    }

    fn on_set_previous_random(&mut self, new_val: i32) {
        self.writes.push(StateWrite::SetPreviousRandom { new_val });
    }
}

/// Maximum `continue_single` calls per episode before aborting.
const STEP_LIMIT: usize = 10_000;

#[expect(clippy::too_many_lines)]
fn explore_inner(
    mut story: Story<DotNetRng>,
    config: &ExploreConfig,
    initial_state: &StateSnapshot,
    episodes: &mut Vec<Episode>,
    mut steps: Vec<StepRecord>,
    choice_path: Vec<usize>,
    depth: usize,
) {
    if episodes.len() >= config.max_episodes {
        return;
    }

    let mut recorder = ExploreRecorder::new();
    let mut step_count = 0;

    // Step one line at a time with continue_single_observed.
    loop {
        step_count += 1;
        if step_count > STEP_LIMIT {
            episodes.push(Episode {
                steps,
                outcome: Outcome::StepLimit { limit: STEP_LIMIT },
                choice_path,
                initial_state: initial_state.clone(),
            });
            return;
        }

        let line = match story.continue_single_observed(&mut recorder) {
            Ok(line) => line,
            Err(e) => {
                episodes.push(Episode {
                    steps,
                    outcome: Outcome::Error(e.to_string()),
                    choice_path,
                    initial_state: initial_state.clone(),
                });
                return;
            }
        };

        let writes = recorder.drain();

        match line {
            Line::Text { text, tags } => {
                steps.push(StepRecord::new(text, tags, StepOutcome::Continue, writes));
                // Keep stepping.
            }

            Line::Done { text, tags } => {
                steps.push(terminal_step(text, tags, StepOutcome::Done, writes));
                // Probe for the deferred "ran out of content" error
                // when the story didn't reach an explicit -> DONE.
                let outcome = classify_done(&mut story, &mut recorder);
                episodes.push(Episode {
                    steps,
                    outcome,
                    choice_path,
                    initial_state: initial_state.clone(),
                });
                return;
            }

            Line::End { text, tags } => {
                steps.push(terminal_step(text, tags, StepOutcome::Ended, writes));
                episodes.push(Episode {
                    steps,
                    outcome: Outcome::Ended,
                    choice_path,
                    initial_state: initial_state.clone(),
                });
                return;
            }

            // Runtime-unreachable until FS-3r (the E052 fence keeps `await`
            // from producing bytecode, so no flow can park). A park is a
            // turn boundary; if one ever surfaced here, record it as a
            // completed turn so exploration terminates cleanly rather than
            // silently dropping the step.
            Line::Suspended { text, tags } => {
                steps.push(terminal_step(text, tags, StepOutcome::Done, writes));
                episodes.push(Episode {
                    steps,
                    outcome: Outcome::Done,
                    choice_path,
                    initial_state: initial_state.clone(),
                });
                return;
            }

            Line::Choices {
                text,
                tags,
                choices,
            } => {
                let presented: Vec<ChoiceRecord> = choices
                    .iter()
                    .map(|c| ChoiceRecord {
                        text: c.text.clone(),
                        index: c.index,
                        tags: c.tags.clone(),
                    })
                    .collect();

                if depth >= config.max_depth || episodes.len() >= config.max_episodes {
                    steps.push(StepRecord::new(
                        text,
                        tags,
                        StepOutcome::Choices {
                            presented: presented.clone(),
                            selected: 0,
                        },
                        writes,
                    ));
                    episodes.push(Episode {
                        steps,
                        outcome: Outcome::InputsExhausted {
                            remaining_choices: presented,
                        },
                        choice_path,
                        initial_state: initial_state.clone(),
                    });
                    return;
                }

                // For each choice, clone and recurse.
                for (i, choice) in choices.iter().enumerate() {
                    if episodes.len() >= config.max_episodes {
                        return;
                    }

                    let mut branch_steps = steps.clone();
                    branch_steps.push(StepRecord::new(
                        text.clone(),
                        tags.clone(),
                        StepOutcome::Choices {
                            presented: presented.clone(),
                            selected: i,
                        },
                        writes.clone(),
                    ));

                    let mut branch_path = choice_path.clone();
                    branch_path.push(i);

                    let mut branch = story.clone();
                    if branch.choose(choice.index).is_err() {
                        continue;
                    }

                    explore_inner(
                        branch,
                        config,
                        initial_state,
                        episodes,
                        branch_steps,
                        branch_path,
                        depth + 1,
                    );
                }

                return;
            }
        }
    }
}
