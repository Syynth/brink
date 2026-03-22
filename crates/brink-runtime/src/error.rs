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
}
