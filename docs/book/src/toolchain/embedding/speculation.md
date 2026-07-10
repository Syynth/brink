# Speculation

A **speculation** runs the story forward from its current state without
committing to it — "what lines would this choice produce?", "what does this
function return right now?" — and then throws the run away. The live story is
never touched.

This is the runtime primitive under a live inspector's watch expressions, an
editor's scratch evaluation, and any "preview this branch" UI. It's built
directly on the sandbox mode from [The State Model](../concepts/state-model.md):
a `Speculation` is a `Mode::Sandbox` fork of the current state. It reads live
values, but every write it makes is diverted into a private throwaway layer and
discarded when the `Speculation` drops. Discarding is the entire cleanup — a
drop, nothing to roll back.

## Starting one

For the common case — speculate from a `Story`'s main flow — call `speculate()`.
It takes `&self`, not `&mut self`: a speculation clones a snapshot of current
state, so starting one can't disturb the story you started it from.

```rust
# extern crate brink_format;
# extern crate brink_runtime;
# use brink_format::Value;
# use brink_runtime::{Budget, FallbackHandler, RuntimeError, SpeculationStep, Story};
# fn demo(story: &Story) -> Result<(), RuntimeError> {
let mut spec = story.speculate();

// Drive it with the same verbs you'd drive a story with — advance, choose,
// go_to_path, eval_function — then let it drop.
let handler = FallbackHandler;
loop {
    match spec.advance(Budget::default(), &handler)? {
        SpeculationStep::Line(line) if line.is_terminal() => break,
        SpeculationStep::Line(_) => {}          // a produced line; keep going
        SpeculationStep::AwaitingExternal => {
            // A deferred external — resolve it and advance again.
            spec.resolve_external(Value::Null);
        }
    }
}
# Ok(())
# }
```

`advance` returns a `SpeculationStep` — either a `Line` (including a terminal
`Done`/`Choices`/`End`) or `AwaitingExternal`, the same pause-and-resume shape
the [external functions](./external-functions.md) chapter describes. `go_to_path`,
`choose`, and `eval_function` mirror their `Story`/`FlowInstance` counterparts,
but act only on the sandboxed copy.

For orchestration layers juggling many flows over distinct worlds (bevy-brink),
`Speculation::fork_from(program, &world, &local, &flow, &line_tables)` is the
flow-level constructor `speculate()` wraps.

## Budgets

A production story runs under generous hardcoded ceilings — a million VM steps,
ten thousand lines a turn. A speculative probe should fail *fast* on
possibly-malformed or adversarial content instead of burning the full production
budget before giving up, so every `advance` takes an explicit `Budget`:

```rust
# extern crate brink_runtime;
use brink_runtime::Budget;

let budget = Budget { steps: 10_000, lines: 50 };
# let _ = budget;
```

`steps` caps a single `advance` call's inner VM loop; `lines` caps the total
lines the speculation may ever produce across all its `advance` calls. The
`Default` (100,000 steps, 1,000 lines) sits well under the production ceilings.
Exhausting either is an `Err` (`StepLimitExceeded` / `LineLimitExceeded`), not a
`SpeculationStep` variant.

## Externals inside a speculation

Diverting *writes* is only half of side-effect safety. If the speculated content
calls `play_sound()` or `deal_damage()`, sandboxing its state won't stop the
sound or the damage — those effects live in your engine, on the far side of an
`EXTERNAL`. A speculation resolves externals through whatever handler you pass to
`advance`, so the gating happens there.

`KindTieredHandler` is a composable handler that wraps your real bindings and
tiers each external by a `PolicyKind`:

- **`PolicyKind::Query`** — read-only, no side effects. Always delegated live, so
  a watch expression can call `enemy_count()` and see the true answer.
- **`PolicyKind::Effect`** — state-changing. Allowed through only when the
  evaluation regime is `EvalContext::Eval` *and* effects are explicitly armed;
  otherwise it resolves to the ink fallback body. A name you don't classify is
  treated as `Effect` — conservative by default.

```rust
# extern crate brink_runtime;
use std::collections::HashMap;
use brink_runtime::{EvalContext, FallbackHandler, KindTieredHandler, PolicyKind};

let bindings = FallbackHandler; // your real &dyn ExternalFnHandler
let kinds = HashMap::from([
    ("enemy_count".to_string(), PolicyKind::Query),
    ("play_sound".to_string(), PolicyKind::Effect),
]);

// Watch regime: queries run live, effects never fire.
let handler = KindTieredHandler::new(&bindings, kinds, EvalContext::Watch, false);
// pass `&handler` to spec.advance(...)
# let _ = handler.report();
```

The runtime stays **manifest-blind** — `PolicyKind` is plain data. The consumer
maps its own external classification (the analyzer's `ExternalKind`, a host
capability manifest, whatever it has) onto the two-way split and hands over the
`name → PolicyKind` table. `EvalContext::Watch` is the conservative regime where
no effect ever fires; `EvalContext::Eval` with `live_effects` armed is the
deliberate two-key gate for the rarer case where an engine→ink evaluation is
permitted to actually do something. `handler.report()` returns which externals
ran live versus fell back, for diagnostics.

## Speculation vs. sessions

Speculation and [sessions](./sessions.md) both let you run a story
non-destructively, but for different purposes. A speculation is a *throwaway
branch off the present* — evaluate, observe, discard, live state untouched. A
session *records the real playthrough* so you can snapshot it, diff two points,
and replay it deterministically. Reach for speculation to preview; reach for a
session to inspect and reproduce what actually happened.
