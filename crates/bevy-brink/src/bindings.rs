//! Ink → engine external-function bindings (synchronous kinds).
//!
//! When an ink story calls an `EXTERNAL` function, the runtime asks an
//! [`ExternalFnHandler`] how to resolve it. This module provides a
//! registry-backed handler ([`BrinkHandler`]) plus app-level registration
//! verbs for the two *synchronous* binding kinds:
//!
//! - **`bind_brink_fn`** — a pure function `Fn(&[Value]) -> impl Into<Value>`.
//!   No World access; resolved inline while the VM steps. Use for math,
//!   formatting, table lookups against captured data.
//! - **`bind_brink_command`** — fire-and-forget: parse the ink args into a
//!   Bevy [`Event`] and trigger it. The event is *buffered* during stepping
//!   (the handler can't touch the World mid-step) and flushed afterward —
//!   via [`BrinkHandler::flush`] on the normal playback path, or fired
//!   directly against the World once the call completes when reached
//!   through [`call_ink_function`]'s exclusive-`&mut World` driver (issue
//!   #1096: this used to fall through to the in-story fallback silently).
//!   Optionally returns a value to ink via [`BrinkCommand::reply`].
//!
//! plus a third, *world-access* kind used by engine→ink calls:
//!
//! - **`bind_brink_query`** — a Bevy system with arbitrary `SystemParam`s
//!   that reads the World and returns a [`Value`]. It can't run inline
//!   while the VM steps, so [`call_ink_function`] (an exclusive-`&mut World`
//!   driver) runs it via `run_system_with` between evaluation suspensions —
//!   letting an ink function called from the engine query anything in the
//!   World, with no upfront declaration.
//!
//! ## Wiring it up
//!
//! ```ignore
//! app.bind_brink_fn::<(), _, _>("clamp01", |args| {
//!     args.first().and_then(Value::as_float).unwrap_or(0.0).clamp(0.0, 1.0)
//! });
//! app.bind_brink_command::<(), PlaySound>("play_sound");
//! ```
//!
//! Then, in the system that drives flows, build a handler from the
//! registry, step the flow with it, and flush:
//!
//! ```ignore
//! let handler = bindings.handler();
//! let mut view = bevy_brink::flow_context_view(&mut globals, &mut ctx);
//! let line = flow.step_one(program, tables, &mut view, &handler, entity, &mut commands)?;
//! handler.flush(&mut commands);
//! ```
//!
//! ## Module layout
//!
//! Split per issue #684 into two responsibilities: [`registration`] is *how
//! engine code registers callable bindings* ([`BrinkBindings`],
//! [`BrinkHandler`], [`BrinkBindingsAppExt`]); [`drive`] is *how the engine
//! drives ink execution and resolves externals* ([`call_ink_function`],
//! [`advance_flow`], [`resolve_pending_externals`]). Both are re-exported
//! here so `bevy_brink::bindings::*` paths (and the crate-root re-exports in
//! `lib.rs`) stay stable regardless of which file an item lives in.

mod drive;
mod registration;
#[cfg(test)]
#[expect(clippy::panic, reason = "tests assert via panic on the error arm")]
mod tests;

pub use drive::{
    BrinkCallError, advance_flow, any_flow_awaiting_external, call_ink_function,
    call_ink_function_value, call_ink_functions, resolve_pending_externals,
};
pub(crate) use registration::TriggerFn;
pub use registration::{
    BrinkArgError, BrinkBindings, BrinkBindingsAppExt, BrinkCommand, BrinkHandler, BrinkQueryInput,
};
