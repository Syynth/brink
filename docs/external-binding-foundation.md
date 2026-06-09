# External-Binding Foundation (Track A)

**Status:** design → implementation. This is the foundation track for
external-function binding across brink's consumers (Rust app, folklore web,
RPG Maker MZ via NW.js, eventual server module). The companion tooling track is
`docs/host-capability-manifest.md` (Track B, deferred). See `docs/decision-log.md`
(2026-06-09) for the split.

Track A is the plumbing the web consumers are actually blocked on. It is
**behavior-preserving for existing stories**: when no bindings are registered,
the runtime behaves exactly as today (unbound externals fall back to their ink
body). No oracle impact — additive serde derives + new public methods only.

## Layering

Primitives live in `brink-runtime` (so the Rust app and the eventual server
module share them); `brink-web` is a thin wasm veneer; `brink-react` and the
RMMZ adapter are siblings on top of `brink-web`.

| Capability | brink-runtime | brink-web |
|---|---|---|
| External handler | `ExternalFnHandler` trait + `continue_*_with` (exist) | `JsHandler` + `bind_external` |
| Variable get/set by name | new `Story::variable` / `set_variable` | `get_var` / `set_var` |
| Save/load | serde on state + versioned envelope helper | `save` / `load` |
| Seeding | `Context::set_rng_seed` (exists) | seed arg on `new` / `reset` |
| Suspend (Phase 2) | `FlowInstance::advance` / `resolve_external` (exist) | awaiting-external line + `resolve_external` |

## What already exists (do not rebuild)

- `Story::continue_single_with(&dyn ExternalFnHandler)` and
  `continue_maximally_with(...)` — the runtime already threads a handler;
  `brink-web` just hardcodes `FallbackHandler` today.
- `ExternalResult::{Resolved, Fallback, Pending}` and the
  `advance → AwaitingExternal` / `resolve_external` pause-resume.
- `Story::into_snapshot()` / `from_snapshot()` — full in-memory state
  capture/restore (`{default flow, context, instances}`).
- `Context::{global, set_global, set_rng_seed}` (index-based), and
  `Program::{global_name, global_count}` for the name↔index map.
- `Value` derives `Serialize, Deserialize`.

## Phase 1 — synchronous bindings

### Runtime
Nothing new — use `continue_single_with` / `continue_maximally_with`.

### brink-web

```rust
impl StoryRunner {
    // Register a synchronous binding. `f(args) -> value`.
    pub fn bind_external(&self, name: &str, f: js_sys::Function);
    pub fn unbind_external(&self, name: &str);
}
```

`StoryRunner` gains `bindings: RefCell<HashMap<String, js_sys::Function>>`
(single-threaded wasm — no Send/Sync concern). `continue_story` /
`continue_single` build a `JsHandler` over that map and call the `_with`
variants.

`JsHandler: ExternalFnHandler`:
- name bound → invoke the JS fn with the args, convert the return to `Value`,
  `ExternalResult::Resolved(value)`.
- name not bound → `ExternalResult::Fallback` (unchanged behavior) unless the
  lenient policy is set (below).
- JS fn throws → `warn` + `ExternalResult::Resolved(Value::Null)` (a registered
  binding throwing is a host bug; don't propagate a JS exception into the VM,
  and don't silently run the ink fallback as if unbound).

This single verb subsumes bevy's `fn` / `command` / `query` — JS has no borrow
constraint, so "pure compute," "fire-and-forget side effect" (return null), and
"read host state" are all just a synchronous callback.

### Unbound-external policy (folklore "degrade, not dead-end")

`StoryRunner::set_unbound_external_policy(lenient: bool)`:
- **Strict (default):** unbound → `Fallback` → runtime uses the ink fallback
  body, else `UnresolvedExternalCall` error. Current behavior.
- **Lenient:** unbound → `Resolved(Null)` → never errors, never runs an ink
  fallback body. For shipping content that may call host verbs a given build
  doesn't know.

The nuanced "use the ink fallback body *if it exists*, else null" needs program
metadata in the handler — that's Track B (manifest). Track A ships the binary
policy.

### Value ↔ JS boundary

`Value` is already `Serialize`/`Deserialize`, so **`serde-wasm-bindgen`** maps
it to/from native `JsValue` with no hand-written conversion: args arrive as a
JS array, the return is read back into `Value`. Only Int/Float/Bool/String/Null
(+ later List) realistically cross as external args/returns; exotic variants
(`DivertTarget`, `TempPointer`, `FragmentRef`, `VariablePointer`) are
VM-internal and won't appear at the boundary — a return that can't map becomes
`Null` with a warning.

> **Reserved decision (needs sign-off):** use `serde-wasm-bindgen` for the
> binding boundary specifically, rather than the JSON-string envelope used by
> the IDE-query surface. Rationale: the boundary is bidirectional and per-call;
> `Value` is already serde-ready so this is the least code. Alternatives:
> JSON-string envelope (consistent, more JS-author boilerplate) or hand-mapping
> `Value↔JsValue` (dependency-free, more Rust code). The IDE-query envelopes are
> unaffected either way.

## Phase 1 — variable get/set by name

### Runtime (`brink-runtime`)

```rust
impl Story {
    pub fn variable(&self, name: &str) -> Option<&Value>;
    pub fn set_variable(&mut self, name: &str, value: Value) -> bool; // false if unknown
}
```

Maps name → index via `Program::global_name`/`global_count`, then
`Context::global`/`set_global`. `set_variable` accepts Int/Float/Bool/String
(ink is dynamically typed; reject pointer/divert/list-from-JS for now).

### brink-web
`get_var(name) -> JsValue` (null if unknown), `set_var(name, value) -> bool`.

## Phase 1 — deterministic seeding

`StoryRunner::new(story_bytes, seed: Option<i32>)` and `reset(seed: Option<i32>)`
call `Context::set_rng_seed` when `Some` (default seed 0 — current behavior).
Touches the `@brink/wasm` `StoryRunnerHandle` wrapper + the book playground
call site (optional arg, backward compatible).

## Phase 1 — save / load (persistence)

### Runtime (`brink-runtime`)
Derive `Serialize, Deserialize` (unconditionally, matching `Value`) across the
state graph: `Context`, `FlowInstance`, `Flow`, `Thread`, `CallStack`/frames,
`OutputBuffer`/`Fragment`/`OutputPart`, `PendingChoice`, `ChoiceDisplay`,
`ChoiceFlags`, `StoryStatus`, `Stats`, and `DefinitionId` (in brink-format).
`StorySnapshot` becomes serializable. The transient `eval: Option<EvalState>`
on `FlowInstance` is **not** persisted (skip — only live mid-call).

A versioned envelope (mirroring the `.brkt` checksum pattern):

```rust
struct SaveEnvelope { version: u16, program_checksum: u64, state: StorySnapshot }
```

`load` validates `program_checksum` against the current program and errors on
mismatch (you can't resume a save against a changed story).

### brink-web
`save() -> Vec<u8>` (clone story → `into_snapshot` → serialize envelope);
`load(bytes)` (deserialize → checksum-check → `from_snapshot`). The persistence
*backend* (localStorage / IndexedDB / RMMZ save slot / Node fs) is the host's;
`brink-react` wires auto-persist.

This is the largest Phase-1 piece (serde across the state graph) and lands
last.

## Phase 2 — suspend / await (designed-in, additive)

For inline timing (`~ camera("bow") ~ wait(2.0) ~ wreck()`), a binding can't
answer synchronously. Reuse the runtime's existing pause-resume:

- An async binding makes the handler return `ExternalResult::Pending`.
- `continue_*` surfaces a new line variant `{ type: "awaiting_external", name,
  args }` instead of erroring.
- JS does its async work, calls `runner.resolve_external(value)`, then resumes
  `continue_*`.

Correlation is trivial (single flow = single pending external — the runner is
the key, like bevy's flow entity). The `@brink/wasm` wrapper hides the
park→await→resolve→continue loop so authors register an `async (args)=>value`.
The sync `bind_external` signature is unchanged — Phase 2 is purely additive.

## Phase 3 — engine → ink (later)

`call_function(name, args) -> value` over `begin_function_eval` /
`resume_function_eval`, resolving sync JS bindings inline (powers a Studio
"evaluate function" panel + host-driven logic). Deferred design-in only.

## Implementation order (one commit each)

1. **brink-web sync bindings** — `JsHandler` + `bind_external` + switch to
   `continue_*_with` + serde-wasm-bindgen Value boundary + unbound policy.
2. **Name-based var get/set** — runtime `Story::variable`/`set_variable` +
   web `get_var`/`set_var`.
3. **Seeding** — `new`/`reset` seed arg + TS wrapper + playground call site.
4. **Save/load** — runtime serde derives + envelope + web `save`/`load`.
5. **(Phase 2) Suspend** — awaiting-external line + `resolve_external` + wrapper
   async sugar.

Each step is independently shippable; `brink-react` and the RMMZ adapter build
on the cumulative surface.
