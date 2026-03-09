//! Episode recording types for behavioral testing.

use brink_format::{DefinitionId, Value};
use serde::{Deserialize, Serialize};

/// A complete recorded execution of a story from start to termination.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Episode {
    /// Per-step records in execution order.
    pub steps: Vec<StepRecord>,
    /// How the episode ended.
    pub outcome: Outcome,
    /// The sequence of choice indices selected during execution.
    pub choice_path: Vec<usize>,
    /// Snapshot of initial state after `Story::new()`.
    pub initial_state: StateSnapshot,
}

/// A single `continue_maximally` call's output and side effects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepRecord {
    /// Text output from this step.
    pub text: String,
    /// Per-line tags.
    pub tags: Vec<Vec<String>>,
    /// What happened at the end of this step.
    pub outcome: StepOutcome,
    /// External function calls made during this step.
    pub external_calls: Vec<ExternalCall>,
    /// State mutations observed during this step.
    pub writes: Vec<StateWrite>,
}

/// The outcome of a single step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StepOutcome {
    /// Story paused — more content may follow.
    Done,
    /// Choices were presented and one was selected.
    Choices {
        presented: Vec<ChoiceRecord>,
        selected: usize,
    },
    /// Story permanently ended.
    Ended,
}

/// A single choice as presented to the player.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceRecord {
    pub text: String,
    pub index: usize,
    pub tags: Vec<String>,
}

/// A record of an external function call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalCall {
    pub name: String,
    pub args: Vec<Value>,
    pub result: ExternalCallResult,
}

/// How an external function call was resolved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExternalCallResult {
    Resolved(Value),
    Fallback,
}

/// A single state mutation observed during execution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StateWrite {
    SetGlobal { idx: u32, value: Value },
    IncrementVisit { id: DefinitionId, new_count: u32 },
    SetTurnCount { id: DefinitionId, turn: u32 },
    IncrementTurnIndex { new_value: u32 },
    SetRngSeed { new_seed: i32 },
    SetPreviousRandom { new_val: i32 },
}

/// Snapshot of story state at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Global variable values.
    pub globals: Vec<Value>,
}

/// How an episode ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Outcome {
    /// Story hit an `end` opcode — permanently finished.
    Ended,
    /// Story paused (no more choices to auto-select).
    Done,
    /// Ran out of pre-supplied choice inputs.
    InputsExhausted {
        remaining_choices: Vec<ChoiceRecord>,
    },
    /// Hit the maximum step limit.
    StepLimit { limit: usize },
    /// Runtime error during execution.
    Error(String),
}
