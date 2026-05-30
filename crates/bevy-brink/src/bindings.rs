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
//!   (the handler can't touch the World mid-step) and flushed afterward via
//!   [`BrinkHandler::flush`]. Optionally returns a value to ink via
//!   [`BrinkCommand::reply`].
//!
//! The third kind — `bind_brink_query`, for bindings that need read access
//! to the Bevy World — is deferred: it needs the async (Pending +
//! exclusive-resolver) path that engine→ink calls also use.
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
//! let line = flow.step_one(program, tables, &mut ctx.inner, &handler, entity, &mut commands)?;
//! handler.flush(&mut commands);
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;

use bevy_app::App;
use bevy_ecs::event::Event;
use bevy_ecs::resource::Resource;
use bevy_ecs::system::Commands;
use bevy_ecs::world::World;
use bevy_log::warn;
use brink_format::Value;
use brink_runtime::{ExternalFnHandler, ExternalResult};
use thiserror::Error;

/// Error produced when ink arguments can't be parsed into a binding's
/// expected shape. Returned by [`BrinkCommand::from_ink_args`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum BrinkArgError {
    /// Wrong number of arguments.
    #[error("expected {expected} argument(s), got {got}")]
    Count {
        /// How many arguments the binding declared.
        expected: usize,
        /// How many ink actually passed.
        got: usize,
    },
    /// An argument had the wrong runtime type.
    #[error("argument {index}: expected {expected}")]
    Type {
        /// Zero-based argument position.
        index: usize,
        /// The type the binding expected (e.g. `"int"`, `"string"`).
        expected: &'static str,
    },
}

/// A Bevy [`Event`] that can be built from an ink external call's
/// arguments, for use with [`bind_brink_command`](BrinkBindingsAppExt::bind_brink_command).
///
/// Implement (or `#[derive(BrinkCommand)]`) this for the event your
/// binding fires. The derive generates [`from_ink_args`](Self::from_ink_args)
/// for structs whose fields are `i32`, `f32`, `bool`, or `String`. To
/// return a value to ink, hand-implement the trait and override
/// [`reply`](Self::reply).
pub trait BrinkCommand: Sized {
    /// Parse the ink call's arguments (in declaration order) into `Self`.
    fn from_ink_args(args: &[Value]) -> Result<Self, BrinkArgError>;

    /// The value handed back to ink as this external's return value.
    ///
    /// Defaults to [`Value::Null`] — the natural "fire-and-forget, no
    /// return" behavior. Override to feed a computed value back into the
    /// story (e.g. a dice roll).
    fn reply(&self) -> Value {
        Value::Null
    }
}

// Type aliases for the boxed registry entries.
type PureFn = Box<dyn Fn(&[Value]) -> Value + Send + Sync>;
type CommandFn = Box<dyn Fn(&[Value]) -> Result<QueuedCommand, BrinkArgError> + Send + Sync>;
/// A deferred World mutation that triggers a parsed command event. Boxed
/// so heterogeneous command types share one buffer; run during flush.
type TriggerFn = Box<dyn FnOnce(&mut World) + Send>;

/// A parsed command ready to be triggered against the World, plus the
/// value to return to ink.
struct QueuedCommand {
    /// Triggers the parsed event when run against the World.
    trigger: TriggerFn,
    /// Value returned to ink (usually [`Value::Null`]).
    reply: Value,
}

/// Registry of synchronous ink→engine bindings for story marker `M`.
///
/// A `Resource`. Populate it at app-build time with
/// [`bind_brink_fn`](BrinkBindingsAppExt::bind_brink_fn) and
/// [`bind_brink_command`](BrinkBindingsAppExt::bind_brink_command), then,
/// in the flow-driving system, call [`handler`](Self::handler) to get a
/// [`BrinkHandler`] to pass to the flow's step methods.
#[derive(Resource)]
pub struct BrinkBindings<M: Send + Sync + 'static = ()> {
    pure: HashMap<String, PureFn>,
    commands: HashMap<String, CommandFn>,
    _marker: PhantomData<fn() -> M>,
}

impl<M: Send + Sync + 'static> Default for BrinkBindings<M> {
    fn default() -> Self {
        Self {
            pure: HashMap::new(),
            commands: HashMap::new(),
            _marker: PhantomData,
        }
    }
}

impl<M: Send + Sync + 'static> BrinkBindings<M> {
    /// Build a [`BrinkHandler`] borrowing this registry. Pass `&handler`
    /// to a flow's step method, then call [`BrinkHandler::flush`] to emit
    /// any buffered command events.
    #[must_use]
    pub fn handler(&self) -> BrinkHandler<'_, M> {
        BrinkHandler {
            bindings: self,
            queued: RefCell::new(Vec::new()),
        }
    }
}

/// An [`ExternalFnHandler`] backed by a [`BrinkBindings`] registry.
///
/// Resolves pure-function bindings inline and buffers command-event
/// triggers (it has no World access mid-step). After stepping, call
/// [`flush`](Self::flush) to drain the buffered triggers into a
/// [`Commands`] queue. Unknown names fall through to
/// [`ExternalResult::Fallback`] so the in-story fallback body (if any)
/// runs.
pub struct BrinkHandler<'a, M: Send + Sync + 'static = ()> {
    bindings: &'a BrinkBindings<M>,
    queued: RefCell<Vec<TriggerFn>>,
}

impl<M: Send + Sync + 'static> BrinkHandler<'_, M> {
    /// Drain buffered command-event triggers into `commands`. Call once
    /// after the flow's step method returns (the borrow of `self` taken
    /// by stepping has ended by then). Consumes the handler.
    pub fn flush(self, commands: &mut Commands) {
        for trigger in self.queued.into_inner() {
            commands.queue(trigger);
        }
    }

    /// Number of command triggers buffered so far (for tests/diagnostics).
    #[must_use]
    pub fn queued_len(&self) -> usize {
        self.queued.borrow().len()
    }
}

impl<M: Send + Sync + 'static> ExternalFnHandler for BrinkHandler<'_, M> {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        if let Some(f) = self.bindings.pure.get(name) {
            return ExternalResult::Resolved(f(args));
        }
        if let Some(parse) = self.bindings.commands.get(name) {
            return match parse(args) {
                Ok(queued) => {
                    self.queued.borrow_mut().push(queued.trigger);
                    ExternalResult::Resolved(queued.reply)
                }
                Err(err) => {
                    warn!("brink command '{name}': {err}; emitting nothing, returning null");
                    ExternalResult::Resolved(Value::Null)
                }
            };
        }
        ExternalResult::Fallback
    }
}

/// App-extension verbs for registering synchronous ink→engine bindings.
///
/// Both verbs take the story marker `M` as the first explicit type
/// parameter (use `()` for the default single-story case). They insert
/// into the [`BrinkBindings<M>`] resource, creating it on first use.
pub trait BrinkBindingsAppExt {
    /// Register a **pure** binding: a side-effect-free function of the ink
    /// arguments that returns a value to the story. Resolved inline while
    /// the VM steps — no World access, no latency.
    ///
    /// The return type is anything `Into<Value>`, so primitives work
    /// directly: `|args| 1.5_f32`, `|args| count as i32`, etc.
    fn bind_brink_fn<M, F, R>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        M: Send + Sync + 'static,
        F: Fn(&[Value]) -> R + Send + Sync + 'static,
        R: Into<Value>;

    /// Register a **command** binding: parse the ink arguments into a Bevy
    /// [`Event`] and trigger it (fire-and-forget). The event is buffered
    /// during stepping and emitted when the handler is flushed. The story
    /// receives [`BrinkCommand::reply`] as the call's return value
    /// (`Value::Null` by default).
    ///
    /// `E` should be a plain `#[derive(Event)]` (a global observer event):
    /// react to it with `app.add_observer(|on: On<E>| { … })`.
    fn bind_brink_command<M, E>(&mut self, name: impl Into<String>) -> &mut Self
    where
        M: Send + Sync + 'static,
        E: Event + BrinkCommand,
        for<'a> <E as Event>::Trigger<'a>: Default;
}

impl BrinkBindingsAppExt for App {
    fn bind_brink_fn<M, F, R>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        M: Send + Sync + 'static,
        F: Fn(&[Value]) -> R + Send + Sync + 'static,
        R: Into<Value>,
    {
        let name = name.into();
        {
            let mut reg = self
                .world_mut()
                .get_resource_or_insert_with(BrinkBindings::<M>::default);
            reg.pure.insert(name, Box::new(move |args| f(args).into()));
        }
        self
    }

    fn bind_brink_command<M, E>(&mut self, name: impl Into<String>) -> &mut Self
    where
        M: Send + Sync + 'static,
        E: Event + BrinkCommand,
        for<'a> <E as Event>::Trigger<'a>: Default,
    {
        let name = name.into();
        {
            let mut reg = self
                .world_mut()
                .get_resource_or_insert_with(BrinkBindings::<M>::default);
            reg.commands.insert(
                name,
                Box::new(move |args: &[Value]| {
                    let event = E::from_ink_args(args)?;
                    let reply = event.reply();
                    Ok(QueuedCommand {
                        trigger: Box::new(move |world: &mut World| {
                            world.trigger(event);
                        }),
                        reply,
                    })
                }),
            );
        }
        self
    }
}

#[cfg(test)]
#[expect(clippy::panic, reason = "tests assert via panic on the error arm")]
mod tests {
    use super::*;
    use crate::test_support::compile_test_story;
    use bevy_ecs::prelude::*;
    use brink_runtime::{FastRng, FlowInstance};

    /// A command event used by tests. `reply` is overridden to echo the
    /// label length back to ink, exercising the value-return path.
    #[derive(Event, Clone, Debug, PartialEq, Eq)]
    struct Ping {
        label: String,
    }

    impl BrinkCommand for Ping {
        fn from_ink_args(args: &[Value]) -> Result<Self, BrinkArgError> {
            let label = args
                .first()
                .and_then(Value::as_str)
                .ok_or(BrinkArgError::Type {
                    index: 0,
                    expected: "string",
                })?
                .to_string();
            Ok(Self { label })
        }

        fn reply(&self) -> Value {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_possible_wrap,
                reason = "test value, small"
            )]
            Value::Int(self.label.len() as i32)
        }
    }

    /// A multi-field command whose `BrinkCommand` impl is generated by the
    /// derive macro (strict types, default `reply` of `Null`).
    #[derive(Event, Clone, Debug, PartialEq, bevy_brink_derive::BrinkCommand)]
    struct SetVolume {
        channel: i32,
        level: f32,
    }

    fn app_with_double_and_ping() -> App {
        let mut app = App::new();
        app.bind_brink_fn::<(), _, _>("double", |args| {
            args.first().and_then(Value::as_int).unwrap_or(0) * 2
        });
        app.bind_brink_command::<(), Ping>("ping");
        app
    }

    #[test]
    fn pure_fn_resolves_inline() {
        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();
        match handler.call("double", &[Value::Int(21)]) {
            ExternalResult::Resolved(Value::Int(42)) => {}
            other => panic!("expected Resolved(Int(42)), got {other:?}"),
        }
        assert_eq!(handler.queued_len(), 0, "pure fn buffers nothing");
    }

    #[test]
    fn command_buffers_trigger_and_returns_reply() {
        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();
        // "hi" has length 2 → reply Int(2); one trigger buffered.
        match handler.call("ping", &[Value::from("hi")]) {
            ExternalResult::Resolved(Value::Int(2)) => {}
            other => panic!("expected Resolved(Int(2)), got {other:?}"),
        }
        assert_eq!(handler.queued_len(), 1, "command buffers one trigger");
    }

    #[test]
    fn unknown_name_falls_back() {
        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();
        match handler.call("nonexistent", &[]) {
            ExternalResult::Fallback => {}
            other => panic!("expected Fallback, got {other:?}"),
        }
    }

    #[test]
    fn bad_command_args_resolve_null_without_buffering() {
        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();
        // ping wants a string; give it an int → parse fails → Null, no buffer.
        match handler.call("ping", &[Value::Int(7)]) {
            ExternalResult::Resolved(Value::Null) => {}
            other => panic!("expected Resolved(Null), got {other:?}"),
        }
        assert_eq!(handler.queued_len(), 0, "failed parse buffers nothing");
    }

    /// End-to-end: a pure-fn binding's return value is inlined into story
    /// text by the real VM.
    #[test]
    fn e2e_pure_fn_value_appears_in_text() {
        let (program, tables, _ctx) =
            compile_test_story("EXTERNAL double(x)\nResult: {double(21)}.\n-> END\n");

        let app = app_with_double_and_ping();
        let bindings = app.world().resource::<BrinkBindings<()>>();
        let handler = bindings.handler();

        let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
        let mut text = String::new();
        loop {
            let line = flow
                .step_single_line::<FastRng>(&program, &tables, &mut ctx, &handler, None)
                .unwrap();
            text.push_str(line.text());
            if line.is_terminal() {
                break;
            }
        }
        assert!(
            text.contains("Result: 42"),
            "expected 'Result: 42' in story text; got {text:?}"
        );
    }

    /// End-to-end: a command binding fires its Bevy event when the VM hits
    /// the external call. We drive the VM, then apply the buffered trigger
    /// to the world and confirm an observer saw the event.
    #[test]
    fn e2e_command_triggers_event() {
        #[derive(Resource, Default)]
        struct PingLog(Vec<String>);

        let (program, tables, _ctx) =
            compile_test_story("EXTERNAL ping(label)\nA{ping(\"hi\")}B\n-> END\n");

        let mut app = app_with_double_and_ping();
        app.init_resource::<PingLog>();
        app.add_observer(|on: On<Ping>, mut log: ResMut<PingLog>| {
            log.0.push(on.event().label.clone());
        });

        // Drive the flow inside a scope so the borrow of BrinkBindings
        // ends before we mutate the world to apply triggers.
        let triggers = {
            let bindings = app.world().resource::<BrinkBindings<()>>();
            let handler = bindings.handler();
            let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
            loop {
                let line = flow
                    .step_single_line::<FastRng>(&program, &tables, &mut ctx, &handler, None)
                    .unwrap();
                if line.is_terminal() {
                    break;
                }
            }
            handler.queued.into_inner()
        };
        assert_eq!(triggers.len(), 1, "exactly one ping trigger buffered");

        for trigger in triggers {
            trigger(app.world_mut());
        }

        let log = app.world().resource::<PingLog>();
        assert_eq!(
            log.0,
            vec!["hi".to_string()],
            "observer should see ping(\"hi\")"
        );
    }

    #[test]
    fn derived_from_ink_args_parses_strictly() {
        // Correct count + types.
        let ok = SetVolume::from_ink_args(&[Value::Int(2), Value::Float(0.5)]).unwrap();
        assert_eq!(
            ok,
            SetVolume {
                channel: 2,
                level: 0.5
            }
        );
        // Default reply is Null.
        assert!(matches!(ok.reply(), Value::Null));

        // Wrong count.
        assert_eq!(
            SetVolume::from_ink_args(&[Value::Int(2)]),
            Err(BrinkArgError::Count {
                expected: 2,
                got: 1
            })
        );

        // Wrong type at index 1 (int where float expected — strict, no
        // coercion, mirroring bladeink's derive).
        assert_eq!(
            SetVolume::from_ink_args(&[Value::Int(2), Value::Int(3)]),
            Err(BrinkArgError::Type {
                index: 1,
                expected: "float"
            })
        );
    }

    /// End-to-end: a derived command binding fires its event when the VM
    /// hits the external call, just like a hand-written one.
    #[test]
    fn e2e_derived_command_triggers_event() {
        #[derive(Resource, Default)]
        struct VolumeLog(Vec<(i32, f32)>);

        let (program, tables, _ctx) =
            compile_test_story("EXTERNAL set_volume(ch, lvl)\nA{set_volume(2, 0.5)}B\n-> END\n");

        let mut app = App::new();
        app.bind_brink_command::<(), SetVolume>("set_volume");
        app.init_resource::<VolumeLog>();
        app.add_observer(|on: On<SetVolume>, mut log: ResMut<VolumeLog>| {
            log.0.push((on.event().channel, on.event().level));
        });

        let triggers = {
            let bindings = app.world().resource::<BrinkBindings<()>>();
            let handler = bindings.handler();
            let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
            loop {
                let line = flow
                    .step_single_line::<FastRng>(&program, &tables, &mut ctx, &handler, None)
                    .unwrap();
                if line.is_terminal() {
                    break;
                }
            }
            handler.queued.into_inner()
        };
        assert_eq!(triggers.len(), 1);
        for trigger in triggers {
            trigger(app.world_mut());
        }

        let log = app.world().resource::<VolumeLog>();
        assert_eq!(
            log.0,
            vec![(2, 0.5)],
            "observer should see set_volume(2, 0.5)"
        );
    }
}
