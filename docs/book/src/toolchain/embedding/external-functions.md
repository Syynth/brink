# External Functions

Ink stories can call functions the host provides — `EXTERNAL fn_name(args)` in
ink source. When the VM hits such a call, it asks your handler for a value.
Implement the `ExternalFnHandler` trait:

```rust,ignore
trait ExternalFnHandler {
    fn call(&self, name: &str, args: &[Value]) -> ExternalResult;
}

enum ExternalResult {
    Resolved(Value),  // return a value immediately
    Fallback,         // run the ink-defined fallback body, if any
    Pending,          // defer resolution — supply the value later
}
```

Step with handler support using the `_with` entry points:

```rust,ignore
let lines = story.continue_maximally_with(&handler)?;
// or, one line at a time:
let line = story.continue_single_with(&handler)?;
```

## Resolution modes

- **`Resolved(value)`** — the common case. You computed the answer; the VM pushes
  it and keeps going.
- **`Fallback`** — defer to the ink-side fallback body declared for that external
  (if the story provides one). Returning `Fallback` for an unknown name is how
  stories stay runnable without every binding present.
- **`Pending`** — you can't answer synchronously (waiting on input, a network
  call, the game world). The story **pauses** on the deferred external. Supply
  the result later with `story.resolve_external(value)` and resume stepping.

```rust,ignore
match story.continue_single_with(&handler)? {
    // … handler returned Pending somewhere inside this step …
    _ => {
        // later, once you have the answer:
        story.resolve_external(Value::Int(42))?;
        let line = story.continue_single_with(&handler)?;
    }
}
```

If you have no externals to provide, pass `&brink_runtime::FallbackHandler` and
every call uses its ink-side fallback.

> The `bevy-brink` integration builds a far richer binding facility on top of
> this — pure / command / world-query / async bindings, plus engine→ink calls.
> See [Bevy › External Functions](../../integrations/bevy/bindings.md).
