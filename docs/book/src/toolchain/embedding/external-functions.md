# External Functions

Ink stories can call functions the host provides — `EXTERNAL fn_name(args)` in
ink source. When the VM hits such a call, it asks your handler for a value.
Implement the `ExternalFnHandler` trait:

```rust
# extern crate brink_format;
# extern crate brink_runtime;
use brink_format::Value;
use brink_runtime::{ExternalFnHandler, ExternalResult};

struct Dice;

impl ExternalFnHandler for Dice {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult {
        match name {
            "roll" => ExternalResult::Resolved(Value::Int(4)),
            // Unknown name — let the story's own fallback body answer.
            _ => ExternalResult::Fallback,
        }
    }
}
```

`ExternalResult` has three variants:

```rust
# extern crate brink_format;
# use brink_format::Value;
# #[allow(dead_code)]
enum ExternalResult {
    Resolved(Value),  // return a value immediately
    Fallback,         // run the ink-defined fallback body, if any
    Pending,          // defer resolution — supply the value later
}
```

Step with handler support using the `_with` entry points, which take
`&dyn ExternalFnHandler`:

```rust
# extern crate brink_format;
# extern crate brink_runtime;
# use brink_runtime::{FallbackHandler, RuntimeError, Story};
# fn demo(story: &mut Story) -> Result<(), RuntimeError> {
# let handler = FallbackHandler;
let lines = story.continue_maximally_with(&handler)?;
// or, one line at a time:
let line = story.continue_single_with(&handler)?;
# let _ = (lines, line);
# Ok(())
# }
```

## Resolution modes

- **`Resolved(value)`** — the common case. You computed the answer; the VM pushes
  it and keeps going. `Value::Null` is valid for fire-and-forget calls.
- **`Fallback`** — defer to the ink-side fallback body declared for that external
  (if the story provides one). Returning `Fallback` for an unknown name is how
  stories stay runnable without every binding present.
- **`Pending`** — you can't answer synchronously (waiting on input, a network
  call, the game world). The story **pauses** on the deferred external, freezing
  with the call frame intact. Supply the result later with
  `story.resolve_external(value)` and resume stepping.

`resolve_external` returns `()`, not a `Result` — resolving a value that nothing
is waiting for is a no-op, so there is nothing to handle:

```rust
# extern crate brink_format;
# extern crate brink_runtime;
# use brink_format::Value;
# use brink_runtime::{FallbackHandler, RuntimeError, Story};
# fn demo(story: &mut Story) -> Result<(), RuntimeError> {
# let handler = FallbackHandler;
// The handler returned `Pending` somewhere inside this step, so the story
// is now parked on the deferred call.
let _line = story.continue_single_with(&handler)?;

// Later, once you have the answer:
story.resolve_external(Value::Int(42));
let _line = story.continue_single_with(&handler)?;
# Ok(())
# }
```

While a flow is parked on an unresolved external, jumping elsewhere with
`choose_path_string` fails with `JumpWhileAwaitingExternal` — a pending host
call can't be silently abandoned.

If you have no externals to provide, pass `&brink_runtime::FallbackHandler` and
every call uses its ink-side fallback.

> Orchestration layers that need to surface a deferred external rather than
> block on it should drive `FlowInstance::advance()`, which returns
> `StepOutcome::AwaitingExternal` instead of erroring. See
> [Runtime API](../reference/runtime-api.md).

> The `bevy-brink` integration builds a far richer binding facility on top of
> this — pure / command / world-query / async bindings, plus engine→ink calls.
> See [Bevy › External Functions](../../integrations/bevy/bindings.md).
