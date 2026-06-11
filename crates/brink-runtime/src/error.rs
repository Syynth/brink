//! Runtime error types.

use brink_format::{DecodeError, DefinitionId};

/// Errors that can occur during story linking or execution.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("bytecode decode error: {0}")]
    Decode(#[from] DecodeError),

    #[error("unresolved definition: {0}")]
    UnresolvedDefinition(DefinitionId),

    #[error("no root container found")]
    NoRootContainer,

    #[error("value stack underflow")]
    StackUnderflow,

    #[error("call stack underflow")]
    CallStackUnderflow,

    #[error("container stack underflow")]
    ContainerStackUnderflow,

    #[error("invalid choice index: {index} (available: {available})")]
    InvalidChoiceIndex { index: usize, available: usize },

    #[error("not waiting for choice")]
    NotWaitingForChoice,

    #[error("story has ended")]
    StoryEnded,

    #[error("unresolved global: {0}")]
    UnresolvedGlobal(DefinitionId),

    #[error("type error: {0}")]
    TypeError(String),

    #[error("division by zero")]
    DivisionByZero,

    #[error("unimplemented opcode: {0}")]
    Unimplemented(String),

    #[error("unresolved external function call: {0}")]
    UnresolvedExternalCall(DefinitionId),

    #[error("output capture underflow (no checkpoint)")]
    CaptureUnderflow,

    #[error("unknown flow: {0}")]
    UnknownFlow(String),

    #[error("flow already exists: {0}")]
    FlowAlreadyExists(String),

    #[error("ran out of content. Do you need a '-> DONE' or '-> END'?")]
    RanOutOfContent,

    #[error("step limit exceeded ({0} steps)")]
    StepLimitExceeded(u64),

    #[error("line limit exceeded ({0} lines in a single turn)")]
    LineLimitExceeded(usize),

    #[error("locale checksum mismatch: expected {expected:#010x}, got {actual:#010x}")]
    LocaleChecksumMismatch { expected: u32, actual: u32 },

    #[error("locale scope not in base program: {0}")]
    LocaleScopeNotInBase(DefinitionId),

    #[error("locale missing scope required by strict mode: {0}")]
    LocaleScopeMissing(DefinitionId),

    #[error(
        "function evaluation yielded (a function called from the engine cannot present choices or end the story)"
    )]
    FunctionYielded,

    #[error("no function evaluation in progress")]
    NotEvaluatingFunction,

    #[error("a function evaluation is already in progress on this flow")]
    AlreadyEvaluatingFunction,

    /// `call_function` was given a name that resolves to no function/knot.
    #[error("function not found: {0}")]
    FunctionNotFound(String),

    /// A function evaluated via the synchronous `call_function` path called an
    /// external whose handler deferred (`Pending`) — it can't be resolved in a
    /// one-shot synchronous call.
    #[error("external '{0}' is async; cannot resolve during a synchronous call_function")]
    AsyncExternalInCall(String),

    /// `choose_path_string` was given a path that resolves to no knot,
    /// stitch, or label.
    #[error("no knot or stitch found at path '{0}'")]
    UnknownPath(String),

    /// `choose_path_string` was called while the flow is parked on an
    /// unresolved external call. A pending host call cannot be silently
    /// abandoned — resolve it (or reset the story) before jumping.
    #[error(
        "cannot jump to '{path}': the flow is parked on unresolved external '{external}' — \
         resolve it before jumping"
    )]
    JumpWhileAwaitingExternal { path: String, external: String },
}
