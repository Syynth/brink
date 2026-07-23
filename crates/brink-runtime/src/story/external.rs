//! External-function-call protocol: [`ExternalResult`], [`ExternalFnHandler`],
//! [`FallbackHandler`], and engine→ink function-evaluation outcomes
//! ([`FunctionEval`]).

use brink_format::Value;

/// Result of an external function handler call.
#[derive(Debug, Clone)]
pub enum ExternalResult {
    /// The handler resolved the call and returned a value.
    /// `Value::Null` is valid for fire-and-forget calls.
    Resolved(Value),
    /// The handler declined — use the ink fallback body if available.
    Fallback,
    /// The handler cannot resolve the call yet (async resolution).
    /// The VM freezes with the `External` frame intact. The caller must
    /// resolve via `story.resolve_external(value)` before continuing.
    Pending,
}

/// Trait for handling external function calls from ink.
///
/// Implement this to provide runtime-injected external function behavior.
/// The orchestration layer calls [`call`](ExternalFnHandler::call) when the
/// VM encounters a `CallExternal` opcode. The handler can resolve the call
/// immediately, decline to handle it (triggering fallback), or in the future,
/// indicate that resolution is pending (async/WASM).
pub trait ExternalFnHandler {
    /// Handle an external function call.
    ///
    /// `name` is the ink-declared function name. `args` are the values
    /// popped from the value stack, in declaration order.
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult;
}

/// Default handler that always falls back to the ink function body.
///
/// Use this as the `handler` argument to [`FlowInstance::step_single_line`]
/// or [`FlowInstance::choose`] when you don't want to provide a custom
/// external-function binding registry. Every external call returns
/// [`ExternalResult::Fallback`], delegating to the in-story fallback
/// container declared on the `EXTERNAL` declaration.
pub struct FallbackHandler;

impl ExternalFnHandler for FallbackHandler {
    fn call(&self, _name: &str, _args: &[Value]) -> ExternalResult {
        ExternalResult::Fallback
    }
}

/// Outcome of an engine→ink function evaluation
/// ([`FlowInstance::begin_function_eval`] / [`resume_function_eval`](FlowInstance::resume_function_eval)).
///
/// Evaluating an ink function from engine code does not advance the
/// player-visible story: its output is isolated and discarded, and the
/// transcript is untouched. The only result is the function's return
/// value — unless the function calls an external that can't be resolved
/// synchronously.
#[derive(Debug, Clone)]
pub enum FunctionEval {
    /// The function returned this value and evaluation is complete.
    /// (Functions with no explicit `~ return` yield [`Value::Null`].)
    Returned(Value),
    /// The function called an external whose handler returned
    /// [`ExternalResult::Pending`] — typically a binding that needs
    /// engine/World access resolved out-of-band. Evaluation is paused
    /// with its full state intact. Inspect the pending call via
    /// [`pending_external_name`](FlowInstance::pending_external_name) /
    /// [`pending_external_args`](FlowInstance::pending_external_args),
    /// supply the result with
    /// [`resolve_external`](FlowInstance::resolve_external), then call
    /// [`resume_function_eval`](FlowInstance::resume_function_eval).
    AwaitingExternal,
}
