//! Episode recording and simple text-output helpers.

use brink_format::{DefinitionId, Value};
use brink_runtime::{DotNetRng, Program, StepResult, Story, WriteObserver};

use crate::episode::{
    ChoiceRecord, Episode, Outcome, StateSnapshot, StateWrite, StepOutcome, StepRecord,
};

/// Configuration for recording an episode.
pub struct RunConfig {
    /// Pre-supplied choice indices (0-indexed).
    pub inputs: Vec<usize>,
    /// Maximum number of `continue_maximally` calls before aborting.
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
#[expect(clippy::too_many_lines)]
pub fn record(program: &Program, config: &RunConfig) -> Episode {
    let mut story = Story::<DotNetRng>::new(program);
    let initial_state = snapshot_initial(&story, program);
    let mut recorder = EpisodeRecorder::new();
    let mut steps = Vec::new();
    let mut choice_path = Vec::new();
    let mut input_idx = 0;

    for _ in 0..config.max_steps {
        let result = story.continue_maximally_observed(&mut recorder);
        let writes = recorder.drain();

        match result {
            Ok(StepResult::Choices {
                text,
                choices,
                tags,
            }) => {
                let presented: Vec<ChoiceRecord> = choices
                    .iter()
                    .map(|c| ChoiceRecord {
                        text: c.text.clone(),
                        index: c.index,
                        tags: c.tags.clone(),
                    })
                    .collect();

                if input_idx >= config.inputs.len() {
                    steps.push(StepRecord {
                        text,
                        tags,
                        outcome: StepOutcome::Choices {
                            presented: presented.clone(),
                            selected: 0,
                        },
                        external_calls: Vec::new(),
                        writes,
                    });
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

                steps.push(StepRecord {
                    text,
                    tags,
                    outcome: StepOutcome::Choices {
                        presented,
                        selected,
                    },
                    external_calls: Vec::new(),
                    writes,
                });

                if let Err(e) = story.choose(selected) {
                    return Episode {
                        steps,
                        outcome: Outcome::Error(e.to_string()),
                        choice_path,
                        initial_state,
                    };
                }
            }
            Ok(StepResult::Done { text, tags }) => {
                steps.push(StepRecord {
                    text,
                    tags,
                    outcome: StepOutcome::Done,
                    external_calls: Vec::new(),
                    writes,
                });
                return Episode {
                    steps,
                    outcome: Outcome::Done,
                    choice_path,
                    initial_state,
                };
            }
            Ok(StepResult::Ended { text, tags }) => {
                steps.push(StepRecord {
                    text,
                    tags,
                    outcome: StepOutcome::Ended,
                    external_calls: Vec::new(),
                    writes,
                });
                return Episode {
                    steps,
                    outcome: Outcome::Ended,
                    choice_path,
                    initial_state,
                };
            }
            Err(e) => {
                return Episode {
                    steps,
                    outcome: Outcome::Error(e.to_string()),
                    choice_path,
                    initial_state,
                };
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

/// Convenience: parse ink.json, convert, link, and record an episode.
pub fn record_from_ink_json(json_str: &str, inputs: &[usize]) -> Episode {
    let ink: brink_json::InkJson = match serde_json::from_str(json_str) {
        Ok(ink) => ink,
        Err(e) => {
            return Episode {
                steps: Vec::new(),
                outcome: Outcome::Error(format!("json parse error: {e}")),
                choice_path: Vec::new(),
                initial_state: StateSnapshot {
                    globals: Vec::new(),
                },
            };
        }
    };

    let data = match brink_converter::convert(&ink) {
        Ok(data) => data,
        Err(e) => {
            return Episode {
                steps: Vec::new(),
                outcome: Outcome::Error(format!("convert error: {e}")),
                choice_path: Vec::new(),
                initial_state: StateSnapshot {
                    globals: Vec::new(),
                },
            };
        }
    };

    let program = match brink_runtime::link(&data) {
        Ok(p) => p,
        Err(e) => {
            return Episode {
                steps: Vec::new(),
                outcome: Outcome::Error(format!("link error: {e}")),
                choice_path: Vec::new(),
                initial_state: StateSnapshot {
                    globals: Vec::new(),
                },
            };
        }
    };

    let config = RunConfig {
        inputs: inputs.to_vec(),
        max_steps: 10_000,
    };
    record(&program, &config)
}

/// Quick text-only output from a program with pre-supplied choice inputs.
pub fn run_text(program: &Program, inputs: &[usize]) -> Result<String, String> {
    let mut story = Story::<DotNetRng>::new(program);
    let mut output = String::new();
    let mut input_idx = 0;

    for _ in 0..10_000 {
        match story
            .continue_maximally()
            .map_err(|e| format!("runtime error: {e}"))?
        {
            StepResult::Done { text, .. } | StepResult::Ended { text, .. } => {
                output.push_str(&text);
                return Ok(output);
            }
            StepResult::Choices { text, choices, .. } => {
                output.push_str(&text);
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

/// Convenience: parse ink.json, convert, link, and run for text output.
pub fn run_text_from_ink_json(json_str: &str, inputs: &[usize]) -> Result<String, String> {
    let ink: brink_json::InkJson =
        serde_json::from_str(json_str).map_err(|e| format!("json parse error: {e}"))?;
    let data = brink_converter::convert(&ink).map_err(|e| format!("convert error: {e}"))?;
    let program = brink_runtime::link(&data).map_err(|e| format!("link error: {e}"))?;
    run_text(&program, inputs)
}
