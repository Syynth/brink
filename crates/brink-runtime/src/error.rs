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

    /// A host **semantic** access (variable get/set, entry lookup, function
    /// eval) targeted a `#@private` definition while visibility enforcement
    /// was on (M-2b, `docs/modules-spec.md` §4 boundary rule 2). The host is
    /// outside every module. Dev tooling (play-from-here) opts out via
    /// [`Story::set_visibility_enforcement`](crate::Story::set_visibility_enforcement).
    /// Persistence (save/load/journal/replay) is unaffected — it never routes
    /// through the enforced surface.
    #[error(
        "'{name}' is #@private and cannot be accessed by the host \
         (dev tooling may override visibility enforcement)"
    )]
    PrivateAccess {
        /// The private definition's name or path, as the host supplied it.
        name: String,
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
    /// A `NameId` (container/address-path name) referenced an index outside
    /// `StoryData::name_table` — malformed bytecode, not an
    /// author-triggerable condition. Caught at link time, before any of the
    /// name is used to build path lookup tables.
    #[error("name id {0} out of range")]
    InvalidNameId(u16),

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

    // ── TM-3 completion: conversion intrinsics (docs/typed-mode-spec.md
    // §4, maintainer ruling 2026-07-13, issue #659) ──────────────────────
    /// `int(x)`/`float(x)` where `x` is a `String` that fails to parse as
    /// the target numeric type. Turn-terminating fault — no
    /// zero-defaulting, no silent garbage (ruling 1: "Parse failure is a
    /// turn-terminating fault... like a missing map key"). Unlike this,
    /// the classic uppercase `INT()`/`FLOAT()` builtins keep their
    /// pre-existing silent-0 legacy behavior (`value_ops::cast_to_int`/
    /// `cast_to_float`) — untouched, oracle-byte-identical, a distinct
    /// code path.
    #[error("cannot parse {input:?} as {target}")]
    ConversionParseFailure { target: &'static str, input: String },
    /// `int(x)`/`float(x)` where `x` is outside the permissive
    /// numeric+bool domain (divert targets, LIST values, arrays, maps,
    /// records) — compile error under `types = strict` (`brink-analyzer`'s
    /// intrinsic typing/domain check), turn-terminating fault under
    /// `types = gradual` (ruling 2).
    #[error("cannot convert a {got} value to {target}")]
    InvalidConversionDomain {
        target: &'static str,
        got: &'static str,
    },

    // ── T1c function values (docs/t1c-spec.md §3/§6, issue #700) ──────────
    /// `call(f, …)` / a direct `f(…)` where the callee value is not a
    /// function value (nor a divert target). Gradual-mode dispatch fault —
    /// "no silent garbage" (spec §3, value-model-spec §11c).
    #[error("cannot call a {0} value as a function")]
    NotCallable(&'static str),
    /// Calling a function value with the wrong number of arguments: the bound
    /// prefix plus the supplied args must exactly equal the target's declared
    /// arity (spec §3). Turn-terminating in gradual mode; strict mode catches
    /// it at compile time (spec §4).
    #[error(
        "function value expects {expected} argument(s), got {got} (bound {bound} + supplied {supplied})"
    )]
    FunctionValueArity {
        expected: usize,
        got: usize,
        bound: usize,
        supplied: usize,
    },
    /// A rehydrated function value's bound env no longer matches the current
    /// signature — a param was renamed, reordered, or re-moded across a
    /// recompile (spec §6). A defined fault, never a silent misbinding.
    #[error("function value no longer matches its target's signature: {0}")]
    FunctionValueRehydrationMismatch(String),
    /// Invoking a function value that `ref`-binds a flow-private (`#@local`)
    /// cell (spec §3). T1c ships this fault instead of creating-flow identity
    /// (#597): a `#@local`-`ref` binding can only be dereferenced safely from
    /// its creating flow, and no creating-flow identity is tracked yet, so the
    /// invocation faults rather than risk a silent cross-flow misbinding. The
    /// payload is the bound cell's name.
    #[error(
        "function value ref-binds flow-private cell `{0}`; cross-flow invocation is a fault in T1c (see #597)"
    )]
    FunctionValueCrossFlowLocal(String),

    // ── T1e path projections (docs/t1e-spec.md §1(2)/§3) ──────────────────
    /// A live path projection's snapshot segments no longer resolve against
    /// the root cell's *current* value at read or write time: a shrunk
    /// array, a removed map key, or a struct field dropped by recompile.
    /// The single ratified turn-terminating fault for every path-invalidation
    /// cause (spec §1(2): "a defined turn-terminating runtime fault — not a
    /// clamp, not UB"). The payload carries the underlying cause (an
    /// `IndexOutOfBounds`/`MapKeyNotFound`/`RecordFieldNotFound`-shaped
    /// message, or a root-resolution failure).
    #[error("projection invalidated: {0}")]
    ProjectionInvalidated(String),
}
