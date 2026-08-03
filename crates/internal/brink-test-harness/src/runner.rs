//! Episode recording and simple text-output helpers.

use brink_format::{DefinitionId, Value};
use brink_runtime::{DotNetRng, Program, Step, Story, WriteObserver};

use crate::episode::{
    ChoiceRecord, Episode, Outcome, StateSnapshot, StateWrite, StepOutcome, StepRecord,
};
use crate::termination::{classify_done, push_terminal};

/// Configuration for recording an episode.
pub struct RunConfig {
    /// Pre-supplied choice indices (0-indexed).
    pub inputs: Vec<usize>,
    /// Maximum number of `continue_single` calls before aborting.
    ///
    /// **Post-#1684 note (#2104):** same per-turn cost as `explorer.rs`'s
    /// `STEP_LIMIT` — a yield with trailing content now costs one extra
    /// `continue_single` call for its bare terminal (`pending_terminal`),
    /// so this cap's headroom shrank by roughly one call per turn versus
    /// the old fused `Line` model. See `STEP_LIMIT`'s doc comment for the
    /// full explanation; it applies here unchanged.
    pub max_steps: usize,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            max_steps: 10_000,
        }
    }
}

/// Internal observer that collects [`StateWrite`] entries.
struct EpisodeRecorder {
    writes: Vec<StateWrite>,
}

impl EpisodeRecorder {
    fn new() -> Self {
        Self { writes: Vec::new() }
    }

    fn drain(&mut self) -> Vec<StateWrite> {
        core::mem::take(&mut self.writes)
    }
}

impl WriteObserver for EpisodeRecorder {
    fn on_set_global(&mut self, idx: u32, value: &Value) {
        self.writes.push(StateWrite::SetGlobal {
            idx,
            value: value.clone(),
        });
    }

    fn on_increment_visit(&mut self, id: DefinitionId, new_count: u32) {
        self.writes
            .push(StateWrite::IncrementVisit { id, new_count });
    }

    fn on_set_turn_count(&mut self, id: DefinitionId, turn: u32) {
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

/// Snapshot the initial state of a story (globals only).
fn snapshot_initial(story: &Story<DotNetRng>, program: &Program) -> StateSnapshot {
    let globals = program.global_defaults();
    // The story was just created, so globals match defaults.
    let _ = story; // story is used to prove it exists; globals come from program.
    StateSnapshot { globals }
}

/// Record a full episode from a program with pre-supplied choice inputs.
///
/// Each step corresponds to one `continue_single_observed` call.
#[expect(clippy::too_many_lines)]
pub fn record(
    program: &std::sync::Arc<Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    config: &RunConfig,
) -> Episode {
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::clone(program), line_tables);
    let initial_state = snapshot_initial(&story, program);
    let mut recorder = EpisodeRecorder::new();
    let mut steps = Vec::new();
    let mut choice_path = Vec::new();
    let mut input_idx = 0;

    for _ in 0..config.max_steps {
        let step = match story.continue_single_observed(&mut recorder) {
            Ok(step) => step,
            Err(e) => {
                return Episode {
                    steps,
                    outcome: Outcome::Error(e.to_string()),
                    choice_path,
                    initial_state,
                };
            }
        };
        let writes = recorder.drain();

        match step {
            Step::Line(line) => {
                steps.push(StepRecord::new(
                    line.text,
                    line.tags,
                    StepOutcome::Continue,
                    writes,
                ));
            }

            Step::Done => {
                push_terminal(&mut steps, StepOutcome::Done, writes);
                // Probe for the deferred "ran out of content" error when
                // the story didn't reach an explicit -> DONE. Matches
                // explorer.rs's classification — see termination.rs.
                let outcome = classify_done(&mut story, &mut recorder);
                return Episode {
                    steps,
                    outcome,
                    choice_path,
                    initial_state,
                };
            }

            // A park (FS-3r) is a terminal turn boundary recorded exactly
            // like a safely-exited `Done`; runtime-unreachable today behind
            // the E052 fence.
            Step::Suspended => {
                push_terminal(&mut steps, StepOutcome::Done, writes);
                return Episode {
                    steps,
                    outcome: Outcome::Done,
                    choice_path,
                    initial_state,
                };
            }

            Step::End => {
                push_terminal(&mut steps, StepOutcome::Ended, writes);
                return Episode {
                    steps,
                    outcome: Outcome::Ended,
                    choice_path,
                    initial_state,
                };
            }

            Step::Choices(choices) => {
                let presented: Vec<ChoiceRecord> = choices
                    .iter()
                    .map(|c| ChoiceRecord {
                        text: c.text.clone(),
                        index: c.index,
                        tags: c.tags.clone(),
                    })
                    .collect();

                if input_idx >= config.inputs.len() {
                    push_terminal(
                        &mut steps,
                        StepOutcome::Choices {
                            presented: presented.clone(),
                            selected: 0,
                        },
                        writes,
                    );
                    return Episode {
                        steps,
                        outcome: Outcome::InputsExhausted {
                            remaining_choices: presented,
                        },
                        choice_path,
                        initial_state,
                    };
                }

                let selected = config.inputs[input_idx];
                input_idx += 1;
                choice_path.push(selected);

                push_terminal(
                    &mut steps,
                    StepOutcome::Choices {
                        presented,
                        selected,
                    },
                    writes,
                );

                if let Err(e) = story.choose(selected) {
                    return Episode {
                        steps,
                        outcome: Outcome::Error(e.to_string()),
                        choice_path,
                        initial_state,
                    };
                }
            }
        }
    }

    Episode {
        steps,
        outcome: Outcome::StepLimit {
            limit: config.max_steps,
        },
        choice_path,
        initial_state,
    }
}

/// Quick text-only output from a program with pre-supplied choice inputs.
pub fn run_text(
    program: std::sync::Arc<Program>,
    line_tables: Vec<Vec<brink_format::LineEntry>>,
    inputs: &[usize],
) -> Result<String, String> {
    let mut story = Story::<DotNetRng>::new(program, line_tables);
    let mut output = String::new();
    let mut input_idx = 0;

    for _ in 0..10_000 {
        let step = story
            .continue_single()
            .map_err(|e| format!("runtime error: {e}"))?;
        output.push_str(step.text());
        match &step {
            // `Suspended` (FS-3r park) is a terminal turn boundary, grouped
            // with the other terminals; runtime-unreachable today. Terminals
            // carry no text of their own — any trailing content already
            // arrived as its own preceding `Step::Line`.
            Step::Done | Step::End | Step::Suspended => return Ok(output),
            Step::Line(_) => {} // keep going
            Step::Choices(choices) => {
                if input_idx >= inputs.len() {
                    return Ok(output);
                }
                let idx = inputs[input_idx];
                input_idx += 1;
                if idx >= choices.len() {
                    return Err(format!(
                        "choice index {idx} out of range (only {} choices)",
                        choices.len()
                    ));
                }
                story
                    .choose(idx)
                    .map_err(|e| format!("choose error: {e}"))?;
            }
        }
    }

    Err("exceeded 10000 steps".into())
}
