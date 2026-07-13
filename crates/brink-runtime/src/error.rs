//! Runtime error types.

use alloc::string::String;

use brink_format::{DecodeError, DefinitionId};

/// Errors that can occur during story linking or execution.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
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

    /// A host-directed entry (`choose_path_string_with_args`) — or a
    /// `call_function` — was given the wrong number of arguments for the
    /// target's declared parameters.
    #[error("'{target}' expects {expected} argument(s), got {got}")]
    ArgCountMismatch {
        /// The knot/stitch/function path or name.
        target: String,
        /// Declared parameter count.
        expected: u8,
        /// Arguments the host supplied.
        got: usize,
    },

    // ── T1b collections (docs/value-model-spec.md §11c) ──────────────
    //
    // Out-of-bounds/missing-key reads and writes are turn-terminating
    // runtime faults — total operations with no silent growth on
    // write-past-end (`docs/t1b-surface-spec.md` §4). Propagating as
    // `RuntimeError` (rather than a special in-band value) is exactly what
    // "turn-terminating" already means in this VM: it unwinds `step()`,
    // ending the current turn, the same mechanism `DivisionByZero` uses.
    /// Array index read/write out of bounds (`0 <= index < len` required).
    #[error("array index {index} out of bounds (len {len})")]
    IndexOutOfBounds { index: i32, len: usize },
    /// Map key read/write on a key that isn't present. Indexed *write*
    /// (`m[k] = v`) requires the key to already exist — it never inserts;
    /// use the `insert()` stdlib mutator (T1b-3) to add a new key.
    #[error("map has no key {key}")]
    MapKeyNotFound { key: String },
    /// `a[i]`/`a[i] = v`/`m[k]`/`m[k] = v` where `a`/`m` isn't an
    /// `Array`/`Map`.
    #[error("cannot index into a {0} value")]
    NotIndexable(&'static str),
    /// Array index expression didn't evaluate to an `Int`.
    #[error("array index must be an int, got {0}")]
    InvalidArrayIndex(&'static str),
    /// Map key expression evaluated to a type outside the ratified key
    /// domain (int/string/bool — value-model-spec §4).
    #[error("map key must be int, string, or bool, got {0}")]
    InvalidMapKeyType(&'static str),
    /// `PushLiteral(idx)` referenced an index outside the literal pool —
    /// malformed bytecode, not an author-triggerable condition.
    #[error("literal pool index {0} out of range")]
    InvalidLiteralIndex(u32),

    // ── TM-4 records (docs/typed-mode-spec.md §6 / value-model-spec §11c) ──
    /// `RecordNew(shape_id)` referenced a shape id outside the compiled
    /// `StructShapes` table — malformed bytecode.
    #[error("struct shape id {0} out of range")]
    InvalidShapeId(u32),
    /// `RecordGetDyn`/`RecordSetDyn` on a value that isn't a `Record`.
    #[error("cannot access a field on a {0} value")]
    NotARecord(&'static str),
    /// `RecordGetDyn`/`RecordSetDyn` named a field the record's shape
    /// doesn't declare — a compile-time typo under strict mode (surfaced as
    /// a diagnostic there) or a genuine dynamic mismatch under gradual mode,
    /// both turn-terminating at runtime (spec §11c pattern).
    #[error("struct has no field {0:?}")]
    RecordFieldNotFound(String),
    /// `RecordGet(offset)`/`RecordSet(offset)` (TM-4c static-offset field
    /// ops) with an offset outside the popped record's own field vector.
    /// These ops never re-check the record's shape (that's the payoff over
    /// `RecordGetDyn`/`RecordSetDyn`) — only the field count is verified, so
    /// this is the sole fault this pair can produce, malformed bytecode or
    /// otherwise.
    #[error("struct field offset {offset} out of range (record has {len} fields)")]
    RecordFieldOffsetOutOfRange { offset: u16, len: usize },
}
