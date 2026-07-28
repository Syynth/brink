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

`StoryRunner::set_seed(i32)` (not a constructor arg — the bytes carry no seed)
applies immediately and is remembered so `reset()` re-applies it for
deterministic replays. Backed by `Story::set_rng_seed` → `Context::set_rng_seed`.
Default unset = runtime default (0). `@brink-lang/web`: `setSeed`.

## Phase 1 — save / load (persistent game state)

**Decided:** persist **game state only**, not execution position — because
execution position (call stack / PC / output-buffer offsets) is inherently
**build-locked** (a recompile shifts indices, so a position snapshot can only
load against the identical program). Game state, keyed by stable identities,
survives story patches. The runtime was designed around this persistent-Context
model. (A future "resume mid-conversation" feature, if wanted, is a separate
build-locked artifact or a version-tolerant choice-replay — out of scope here.)

### Format (`brink-format`) — the durable, name-keyed `SaveState`

A purpose-built wire type (not `Context`'s internal form), so it's
version-tolerant and doesn't leak VM representation:

```rust
struct SaveState {
    version: u16,                          // save-FORMAT version
    globals: BTreeMap<String, Value>,      // by NAME (deterministic order)
    visits: Vec<VisitEntry>,               // by scope DefinitionId, + advisory path
    turns:  Vec<VisitEntry>,
    turn_index: u32, rng_seed: i32, previous_random: i32,
}
struct VisitEntry { id: DefinitionId, path: Option<String>, count: u32 }
struct LoadReport {
    unknown_globals: Vec<String>,
    unresolved_renames: Vec<String>,   // M-3 #@was teaching messages
    anonymous_states_dropped: u32,     // issue #1674 — see below
}
```

Globals are name-keyed (readable, patch-tolerant). Visit/turn counts are keyed
by **`DefinitionId`** — *not* path — because `def_path` only resolves *named*
scopes, so path-keying would silently drop counts for anonymous counted
containers (gathers, choice points), which are semantically real. `DefinitionId`
is a recompile-stable hash of the path and serializes as a `"$tt_hash"` string;
an advisory `path` is attached for named scopes (cosmetic, for dev inspection).
Lives in `brink-format` (already has `serde` + `Value` + `DefinitionId`) so the
runtime stays serde-free.

### Runtime (`brink-runtime`)
```rust
impl Story {
    fn save_state(&self) -> SaveState;                 // capture default flow's context
    fn load_state(&mut self, &SaveState) -> LoadReport; // reconcile
}
```
`load_state` reconciles, version-tolerantly: globals matched by name (unmatched
→ reported in `LoadReport`, since they're genuinely dropped — no slot); visit/
turn counts applied by id (scopes the program lacks are **retained**, not
dropped, so nothing to report) — **except** an anonymous scope's entry (no
author path — a gather, choice point, or sequence with no `(label)`), which
can never be recovered through the `#@was` alias table the way a named miss
can (an alias is only ever written against a name): that miss is counted in
`LoadReport::anonymous_states_dropped` (issue #1674) so the bounded exposure
— a once-only choice may reappear, a sequence may restart — is legible
rather than silent. No `program_checksum` gate — gating would defeat
patch-tolerance.

### brink-web — two transports over the one format
- `save() -> String` / `load(json) -> LoadReport-json` — JSON (dev, inspectable).
- `save_bytes() -> Vec<u8>` / `load_bytes(bytes) -> LoadReport-json` — MessagePack
  (struct-map mode, via `rmp-serde`; release, compact, still field-name-tolerant).

`@brink-lang/web`: `save()→SaveState` (typed) / `saveBytes()→Uint8Array`,
`load(SaveState)→LoadReport` / `loadBytes(Uint8Array)→LoadReport`. The
persistence *backend* (localStorage / RMMZ slot / Node fs) is the host's;
`brink-react` wires auto-persist.

## Phase 2 — suspend / await (✅ landed)

For inline timing (`~ camera("bow") ~ wait(2.0) ~ wreck()`), a UI awaiting a
click, or a `fetch`, a binding can't answer synchronously. Built on the
runtime's existing pause-resume, **unified** with the sync surface:

- `bindExternal(name, fn)` is unchanged — if `fn` returns a **Promise**, the
  flow suspends (no separate `bindAsync`). `JsHandler` detects a thenable return
  (`is_instance_of::<js_sys::Promise>`), stashes it, and returns
  `ExternalResult::Pending`.
- Runtime: `Story::advance_with(handler) -> StepOutcome` surfaces
  `AwaitingExternal` (vs. `continue_*_with`, which error), plus
  `pending_external_name`/`args` delegations.
- brink-web: `advance_one()` returns a `{ type: "awaiting_external", name }`
  JSON line on suspend; `take_pending_promise()` hands the Promise to JS;
  `resolve_external(value)` feeds the awaited result back. The sync
  `continue_story`/`continue_single` are unchanged (they still error on a
  suspending binding — use the async path).
- `@brink-lang/web`: `continueStoryAsync()` / `continueSingleAsync()` drive
  advance→`take_pending_promise`→`await`→`resolve_external`→resume, so authors
  just `await runner.continueStoryAsync()` with `async (args) => …` bindings.
  Low-level `advanceOne`/`takePendingPromise`/`resolveExternal` exposed for
  custom loops.

Correlation is trivial (single flow = single pending external — the runner is
the key). **Rejection**: `continueStoryAsync` resolves the external with `null`
to unstick the flow, then rethrows so the host sees the error. **Reentrancy**:
a `busy` guard turns a binding callback that re-enters a stepping method (or a
second concurrent `continueAsync`) into a `console.warn` + clean `JsError`
rather than a `RefCell` panic.

## Phase 3 — engine → ink (✅ landed)

`Story::call_function(name, args, handler) -> Value` over `begin_function_eval`,
on the default flow: output isolated, visible story untouched, synchronous.
Externals the function calls resolve through the (synchronous) handler inline; a
called external that defers (`Pending`) yields `RuntimeError::AsyncExternalInCall`
after cleaning up the paused eval (sync `call_function` can't await — matching
bevy-brink's `AsyncExternalUnsupported`). brink-web `call_function(name, args)`
takes/returns native JS values; `@brink-lang/web` `callFunction(name, ...args)`.
Single-flow (the default flow); multi-flow is an additive future option.
Powers a Studio "evaluate function" panel + host-driven logic.

## Implementation order (one commit each)

1. ✅ **brink-web sync bindings** — `JsHandler` + `bind_external` + switch to
   `continue_*_with` + hand-mapped `Value`↔JS boundary + unbound policy.
2. ✅ **Name-based var get/set** — runtime `Story::variable`/`set_variable`
   (+ `Program::global_index`, `global_name`/`global_count` ungated) +
   web `get_var`/`set_var`.
3. ✅ **Seeding** — `Story::set_rng_seed`, web `set_seed` (reset-stable).
4. ✅ **Save/load** — `SaveState`/`LoadReport` in brink-format, `Story::save_state`/
   `load_state`, web `save`/`save_bytes`/`load`/`load_bytes` (JSON + MessagePack).
5. ✅ **(Phase 3) engine→ink `call_function`** — `Story::call_function` +
   web `call_function` (native-JS args/return), sync, single-flow.
6. ✅ **(Phase 2) Suspend** — async ink→engine via unified Promise-detecting
   `bindExternal`: handler returns `Pending`, `advance_one` surfaces an
   `awaiting_external` line, `@brink-lang/web` drives park→await→resolve→resume;
   reentrancy guard + rejection handling.

All slices landed with host (`bevy-brink`) + `wasm-bindgen-test` (`brink-web`,
run via `wasm-pack test --node`) coverage, and the `@brink-lang/web` /
`@brink/wasm-types` surface typechecks via brink-studio. Each step is
independently shippable; `brink-react` and the RMMZ adapter build on the
cumulative surface. **Track A is complete.**
