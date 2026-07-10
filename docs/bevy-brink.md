# bevy-brink: Status

Bevy 0.18 integration for the brink ink runtime. Lets a Bevy game load
`.ink` (or `.inkb`) story files, advance them in response to input,
render the produced lines via observer events, and (in dev mode) live-edit
the source file with the running flow rebuilt against the new bytecode.

This document is the source of truth for "where is the bevy-brink work
currently parked." Update it when you make a change that lands.

> **F6.2 superseded the per-flow-`World`-clone model described in several
> sections below** (the "Globals = resource" bullet immediately following,
> and "Fork mode for per-flow context" further down). As of F6.2, every
> flow under a marker shares **one** `BrinkGlobals<M>` `World`; a flow's
> `BrinkContext<M>` holds only a private `FlowLocal` override layer
> (empty by default). What's shared vs. private is a per-`World` policy
> (`WorldPolicy`, installed via `BrinkPlugin::with_policy`), not a
> per-flow clone-or-share choice. See `docs/scoped-flow-state-spec.md`
> (the core model) and its "F6 AMENDMENT" section (the bevy-specific
> rulings) for the design; those sections below are kept as historical
> record of the design that preceded it, not current behavior.
>
> **F6.3 landed** (per-entity `SaveState` durability): `BrinkGlobals::save_state`/
> `load_state` and the free `save_flow_state`/`load_flow_state` compose a save as
> one `SaveState` for the shared `World` plus one per entity flow, host-side — see
> `docs/book/src/integrations/bevy/localization.md`'s "Game-state saves" section
> and `crates/bevy-brink/examples/book_saves.rs`. F6 (#441) is now complete.

## Goal

Make ink stories first-class Bevy assets:

- **Story = asset** (immutable bytecode + line tables, loaded once).
- **Flow = entity-shaped state** (per-NPC / per-conversation execution
  state — call stacks, output buffer, transcript, current PC).
- **Globals = resource** (story-wide variables; shared across flows for
  the same marker by default).

Multi-story support via a ZST marker generic (`BrinkPlugin<M = ()>`).

## Architecture

### Three-asset bundle

A single ink story load produces three labeled sub-assets, bundled in
a top-level `BrinkStoryAsset`:

| Asset                  | Contents                              | Notes |
|------------------------|---------------------------------------|-------|
| `ProgramAsset`         | linked `Program` (bytecode, `find_address`) | shareable across flows |
| `LineTablesAsset`      | `Vec<Vec<LineEntry>>`                 | swappable for locale; future `.inkl` overlay loader produces standalone instances |
| `InitialGlobalsAsset`  | post-init `Context`                   | captured by the loader's init pass; used to seed `BrinkGlobals<M>` for named-knot flows |

Both `.inkb` and `.ink` loaders emit this trio.

### Request-component spawn pattern

```rust
commands.spawn(
    BrinkFlowRequest::<MyStory>::builder()
        .story(asset_server.load("dialogue.ink"))
        .start(FlowStart::Address("intro".into()))
        .build(),
);
```

A plugin-managed system (`fulfill_flow_requests<M>`) waits for the
story's sub-assets to load, builds a `FlowInstance` at the requested
start address, replaces the request component with `BrinkFlow<M>` +
`BrinkProgram<M>`, and inserts `BrinkGlobals<M>` if not already present.

No polling, no readiness latches in user code.

### Observer events

The `BrinkFlow::advance_until_terminal` / `step_one` methods queue
observer events via `commands.trigger`. All four are `EntityEvent`s
targeting the flow's entity, so consumers can either register a global
observer via `app.add_observer(...)` or a per-entity observer via
`world.entity_mut(flow).observe(...)`:

- `BrinkLineDelivered<M>` — a `Line::Text` with text + tags
- `BrinkChoicesPresented<M>` — choices + leading text
- `BrinkTurnDone<M>` — `Line::Done` reached, carries any text accumulated
  this turn
- `BrinkStoryEnded<M>` — `Line::End` reached, carries any text accumulated
  this turn

Important: terminal variants (`Done`, `Choices`, `End`) carry their
own accumulated text — they don't emit a separate preceding
`BrinkLineDelivered` for that text. UIs that just want "everything that
happened this turn" should append the terminal event's `text` field too,
not only the per-line events.

### Init pass

`InkLoaderSettings { run_init: true }` (default) runs the story's
top-of-file setup at load time and captures the resulting `Context`.
Stories whose init code calls host-provided externals can opt out via
`run_init: false`.

### External-function bindings (ink ↔ engine)

The binding facility connects ink `EXTERNAL` functions to engine code and
lets engine code evaluate ink functions. The boundary is split by **World
access**, not by sync/async return shape (see the decision log).

**ink → engine** — register at app build via `BrinkBindingsAppExt` (stored
in the `BrinkBindings<M>` resource):

| Verb | Shape | Resolution |
|------|-------|------------|
| `bind_brink_fn(name, Fn(&[Value]) -> impl Into<Value>)` | pure compute, no World | inline while the VM steps |
| `bind_brink_command::<M, E>(name)` | parse args into a `#[derive(Event, BrinkCommand)]` and trigger it; optional `BrinkCommand::reply` value back to ink | buffered during the step, flushed after |
| `bind_brink_query(name, system)` | a Bevy system `In<BrinkQueryInput> -> Value` with arbitrary `SystemParam`s | flow pauses (`Pending`); a driver runs it via `run_system_with` between suspensions, then resumes |
| `bind_brink_async(name)` | the **async event primitive**: no behavior, just "pause + hand off" | flow parks; `BrinkExternalAwaited<M>` fires (once) at the flow entity; engine resolves across frames via `commands.resolve_brink_external` |
| `bind_brink_task(name, \|args\| async move { … })` | **async task sugar** over `AsyncComputeTaskPool` | flow parks; bevy-brink spawns the `Send + 'static` future and resolves with its output when it finishes (`poll_brink_tasks`) |

`In<BrinkQueryInput>` = `(Entity, Vec<Value>)` — the calling flow entity
plus the ink args. Query bindings can read anything in the World with no
upfront declaration.

**async (defer-across-frames) bindings** — for externals that can't resolve
in one pass. The flow parks on the pending external (the runtime's
`AwaitingExternal`/`resolve_external` pause/resume) and is frozen until
resolved, so the **flow entity is the correlation key** — no per-call entity
or id (unlike `brink_call`). Two flavors:

- `bind_brink_async` — a multi-frame **World interaction** (e.g. `pick_target()`
  opens a targeting UI, waits for a click). On park, `BrinkExternalAwaited<M>`
  (an `EntityEvent` carrying `name` + `args`) fires once at the flow; an
  observer does the work and calls `commands.resolve_brink_external::<M>(flow,
  value)` whenever it's ready (guarded by `has_pending_external`, so stale /
  double resolves are safe no-ops).
- `bind_brink_task` — a detached **off-thread** future (heavy compute, IO,
  network). The future is `Send + 'static` and **cannot access the World** — it
  computes from the ink args only. bevy-brink spawns it on
  `AsyncComputeTaskPool`, parks a `BrinkPendingTask<M>`, and `poll_brink_tasks`
  resolves the flow when it completes.

Async bindings are a **`step_one`-path feature**. The one-pass exclusive
drivers (`advance_flow`, `call_ink_function`) can't await across frames, so on
an async external they return `BrinkCallError::AsyncExternalUnsupported`.

**engine → ink** — evaluate an ink function out-of-band (output isolated,
transcript untouched, visit counts not bumped):

- `call_ink_function::<M>(&mut World, entity, name, args) -> Result<Value, BrinkCallError>`
  — synchronous, from an exclusive system. Resolves world-access query
  bindings inline (it has `&mut World`).
- `commands.brink_call::<M>(flow, name, args).observe(|on: On<BrinkCallResolved>| …)`
  — deferred, from a normal (non-exclusive) system. Spawns a per-call
  entity; the plugin's resolver evaluates and fires `BrinkCallResolved` /
  `BrinkCallFailed` **scoped to that entity**, so a result can never be
  mis-correlated with another call. `IntoBrinkArgs` accepts `()`, tuples of
  `Into<Value>`, `Vec<Value>`, or `&[Value]`.

**playback with inline world queries** — for a story line like
`Enemies near: {enemy_count()}.`:

- `advance_flow::<M>(&mut World, entity) -> Result<Line, BrinkCallError>`
  — exclusive single-line driver; resolves query bindings inline in one
  frame (the playback counterpart to `call_ink_function`).
- non-exclusive `BrinkFlow::step_one` returns `Advance::{ Line, AwaitingQuery }`;
  on `AwaitingQuery` the driver skips the flow (`has_pending_external`) and
  the plugin's `resolve_pending_externals` system (gated on
  `any_flow_awaiting_external`) services it across frames — resolving a query
  inline, firing `BrinkExternalAwaited` for an async-event binding, or spawning
  the task for a task binding.

Why pause/resume rather than resolve inline: the live eval holds
`&mut flow`/`&mut ctx`, which conflicts with the `&mut World` that
`run_system_with` needs. Releasing the borrows at the pause point is the
borrow-safe way to run a registered system mid-evaluation.

Runnable demos (headless): `cargo run --example engine_bindings` exercises the
sync bindings + engine→ink calls; `async_task` and `async_external` cover the
two async flavors. `play_story` covers `bind_brink_fn`/`bind_brink_command` in
an interactive window.

## What is implemented

- ✅ `Rc → Arc` swap in brink-format/brink-runtime so `Program`, `Context`,
  `FlowInstance` are `Send + Sync`. Perf cost: ~3% on typical workloads,
  ~21% on extremely list-heavy / fork-heavy workloads. Still ~17x faster
  than the C# reference at hanoi-10.
- ✅ Three-asset bundle (above) with both loaders.
- ✅ `.inkb` loader (always available).
- ✅ `.ink` loader (gated on `dev` cargo feature) — async BFS over the
  INCLUDE graph via `LoadContext::read_asset_bytes`, sync compile from
  the resulting in-memory cache.
- ✅ `Program::find_address(path)` in brink-runtime for qualified-path →
  container lookup. Resolves knots, qualified stitches (`knot.stitch`),
  and — for compiler-built stories — author labels (`knot.label`,
  `knot.stitch.label`) via the `address_paths` table emitted by
  `brink-codegen-inkb`. Converter output resolves knot/stitch scopes only
  (label parity deferred).
- ✅ `BrinkFlowRequest<M>` with `bon` builder + `fulfillment_system`.
- ✅ `BrinkPlugin<M>` and `BrinkAssetsPlugin`. `BrinkPlugin<M>` is
  marker-parameterized; `BrinkAssetsPlugin` registers types and is
  auto-added once per app.
- ✅ Observer events (above) + `BrinkFlow::step_one` and
  `BrinkFlow::advance_until_terminal` that fire them.
- ✅ Init pass (`run_init_pass` + `InkLoaderSettings`).
- ✅ Dev-mode `BrinkReplayLog` component + `replay_on_reload` system
  (current behavior **unverified** — see "Issues" below).
- ✅ Visual example at `crates/bevy-brink/examples/play_story.rs` — UI
  window with text + choices, SPACE to advance, digit keys to choose.
- ✅ **External-function binding facility** (above): `bind_brink_fn` /
  `bind_brink_command` (+ `#[derive(BrinkCommand)]`) / `bind_brink_query`,
  `call_ink_function`, `commands.brink_call(...).observe(...)`,
  `advance_flow`, and the non-exclusive `step_one` → `Advance` pause/resume
  with the `resolve_pending_externals` plugin system.
- ✅ **Async (defer-across-frames) bindings**: `bind_brink_async` (the
  `BrinkExternalAwaited` + `resolve_brink_external` event primitive) and
  `bind_brink_task` (`AsyncComputeTaskPool` task lifecycle via
  `BrinkPendingTask` + `poll_brink_tasks`). Examples: `async_external`,
  `async_task`.
- ✅ Per-flow `Context` on a `BrinkContext<M>` component (not a shared
  resource); `BrinkGlobals<M>` is the commit target. Opt-in
  `BrinkTranscript<M>` auto-renders from the flow's runtime transcript.
- ✅ Headless example `crates/bevy-brink/examples/engine_bindings.rs`
  demonstrating the engine↔ink facility end-to-end.

## What is verified working (by user observation)

- Loading a `.ink` story via the asset server.
- Window opens; first page of dialogue renders.
- SPACE advances; digit keys pick choices.
- File watcher detects edits to the story or any INCLUDE'd file.

## Open issues

### 1. ~~Hot-reload visual behavior (parked)~~ — FIXED (2026-04-28, visually confirmed)

When the file watcher fires after an edit, the plugin reset+replays the
flow against the new bytecode. The attempted UX has gone through several
iterations:

- **First attempt**: replay fired observer events for every step it
  walked. Side effect: the *consumed* `Choices` events left the UI's
  choice list populated, but the flow was past them — clicking a digit
  produced "choose error: not waiting for choice".
- **Second attempt** (commit `dfb548d7`): replay walks silently, then
  fires events only for the post-replay current page via
  `advance_until_terminal`. User reported this version "didn't render
  anything after the reload."

**Diagnosis (2026-04-28)**: the symptom was actually "renders OLD
text", not "renders nothing." `replay_on_reload` walked the new program
against a stale `BrinkLineTables<M>` resource — the resource is set by
`fulfill_flow_requests` once at fulfillment, and that system is gated
`Without<BrinkFlow<M>>` so it never re-runs to refresh. Sub-asset
labels are stable across reloads, so `Assets<ProgramAsset>::get_mut`
returns the new program, but the line tables resource still pointed to
the old strings. New string IDs in the new program either resolved to
old strings or to nothing depending on overlap.

**Fix**: `replay_on_reload` now re-reads `BrinkLineTables<M>` from
the bundle's current `LineTablesAsset` before walking. See
[replay.rs](../crates/bevy-brink/src/replay.rs) — added `Res<Assets<BrinkStoryAsset>>`
and `Res<Assets<LineTablesAsset>>` system params and a refresh step
right after the modified-event drain.

**Verified**: `flow::tests::hot_reload_with_new_content_renders_new_text`
in `flow.rs` reproduces a real content swap (recompiles the story with
different text and replaces both `ProgramAsset` and `LineTablesAsset`
contents in place) and asserts the rendered output (via a UI render
harness that mirrors `play_story.rs`'s observer wiring) contains the
NEW story text and not the OLD.

**Visual confirmation (2026-04-28)**: ran `cargo run --example play_story`,
edited `crates/bevy-brink/examples/assets/story.ink`, saved, and the
window updated to show the new content with the "Reloaded" banner.
Loader logs confirmed the file watcher fired and `replay_on_reload`
completed without errors.

### 2. ~~`commands.trigger` observer dispatch — unreliable in tests~~ — RESOLVED

The previous hypothesis (`Event` derive on a generic `<M>` interacting
poorly with `Trigger<'a>: Default`) was wrong. Diagnosed 2026-04-28:

- A minimal smoke test (`smoke_commands_trigger_fires_observers` in
  `flow.rs`) confirms `commands.trigger` from a system fires observers
  reliably with the generic `BrinkLineDelivered<()>`.
- The flow tests were failing for two unrelated reasons:
  1. **Test expectations didn't match the runtime API.** Terminal `Line`
     variants (`Done`, `Choices`, `End`) carry their accumulated text in
     their *own* `text` field — they don't emit a separate preceding
     `Line::Text`. The test that fed the runtime `"goodbye\n-> END\n"`
     and expected a `BrinkLineDelivered("goodbye")` event was wrong;
     the runtime delivers it as `BrinkStoryEnded { text: "goodbye\n" }`.
  2. **`FlowStart::Root` does not auto-enter named knots.** A test that
     placed all content under `=== start ===` and used the default
     `FlowStart::Root` produced an empty `Done` and never reached the
     choice. Content has to live at root or the request must specify
     `FlowStart::Address("start")`.

While diagnosing this, the `Event` derive was switched to `EntityEvent`
(every event carries `entity: Entity`, so it's the right idiomatic
choice — and it opens up per-entity observers via `entity.observe(...)`).
This change is not load-bearing for dispatch; it's purely an API-quality
upgrade.

**Implication for hot-reload (issue #1):** the "blank page after reload"
symptom is *not* a dispatch problem either. Possible causes to revisit
with that constraint removed: observer-vs-system ordering inside the
reload tick, an asset-event timing issue (replay running before the new
`ProgramAsset` is committed), or genuinely empty post-replay state for
some story shapes. Needs fresh investigation.

## Implemented (intl)

- ✅ **`.inkl` overlay loader + global, event-driven locale switching**
  (`src/locale.rs`). `InklLoader` loads `.inkl` → `LocaleAsset`;
  `apply_locale_overlay` is the primitive (base + overlay → localized
  `LineTablesAsset`). `BrinkCurrentLocale<M>` holds the active locale;
  `Commands::set_brink_locale::<M>(Some(handle))` sets it and fires
  `BrinkLocaleChanged<M>`; an observer reconciles every flow's `BrinkLocale`
  (cached/shared per `(base, locale)` via `LocalizedTablesCache`), the
  transcript re-renders automatically. New flows read the locale at spawn;
  `catch_up_loaded_locales` handles `.inkl`s that load after a switch.
  `BrinkLocaleOverride<M>` opts a flow out (polyglot NPCs). Demoed in
  `examples/locale_switch.rs`.
- ✅ **`.brkt` transcript persistence** (`src/brkt.rs`). `capture_transcript`
  serializes a flow's output history to `.brkt` bytes (the visible-history
  half of a save file / a story-log mechanic / QA capture); `TranscriptAsset`
  + `BrktLoader` load saved bytes; `render_transcript_asset` re-renders a
  loaded transcript against a program + locale (checksum-validated), so a
  saved history localizes too. Demoed in `examples/transcript_save.rs`.
- ✅ **Async (defer-across-frames) external bindings** (`src/async_bind.rs`).
  `bind_brink_async` parks a flow and fires `BrinkExternalAwaited<M>` (the
  event primitive; resolved by `commands.resolve_brink_external`);
  `bind_brink_task` spawns a `Send + 'static` future on `AsyncComputeTaskPool`
  and resolves the flow when it finishes (`BrinkPendingTask` + `poll_brink_tasks`).
  The flow entity is the correlation key. Demoed in `examples/async_external.rs`
  and `examples/async_task.rs`.

## Deferred (not started)

In rough priority order:

- **Converter label-path addressing** — the compiler now emits an
  `address_paths` table so `find_address` resolves `knot.label` /
  `knot.stitch.label` for compiled stories; the converter still emits an
  empty table (knot/stitch scopes only). Bring the converter to parity if
  a `.ink.json`-sourced story ever needs label addressing.

Done since this list was written: the external-function binding facility
(was "most important"), a pausable stepping primitive (`advance` /
`StepOutcome`) superseding the proposed `step_until_terminal`, and
async-task / defer-across-frames bindings (the former first item here).
- ~~Fork / isolated context per flow~~ — done, but not as sketched here:
  see the F6.2 note at the top of this document (a per-`World` policy +
  per-flow `FlowLocal`, not a per-flow `Context` clone).

## Test status

Inline test modules + `crates/bevy-brink/src/test_support.rs`:

| Module                                | Passing | Failing |
|---------------------------------------|---------|---------|
| `program::find_address_tests`         | 3       | 0       |
| `asset::run_init_pass_tests`          | 3       | 0       |
| `request::tests` (fulfillment)        | 5       | 0       |
| `flow::tests` (advance + events)      | 4       | 0       |
| `source_loader::tests`                | 3       | 0       |

## Decisions captured (see `docs/decision-log.md`)

- bevy-brink loading modes (dev = `.ink`, release = `.inkb`+`.inkl`)
- Expose runtime primitives for direct orchestration (don't kill `Story`)
- Marker-parameterized bevy types
- Default bevy wiring: Resources for shared state, Components for flows
- brink-runtime stays bevy-free

## Design rationale

The decision log captures the formally-approved decisions; this section
records design discussions that informed the shape of the API but
weren't logged as standalone decisions. Read this to understand *why*
the API took the form it did.

### Story-vs-flow split

The reference `bevy_bladeink` integration models ink as "one global
story per app" — a single `InkStory` resource, an implicit single
flow. We deliberately chose **multi-flow**: a story is asset-shaped
(immutable bytecode + line tables, loaded once), and each conversation
is entity-shaped (per-NPC / per-cutscene mutable state — call stacks,
output buffer, transcript).

Rationale: real games run many concurrent dialogues (background NPCs,
cutscenes, side conversations). Forcing them through a single global
story object means either serializing every interaction or rebuilding
the story object per-NPC. Per-flow entities let many flows share one
program and one set of globals (the common case) while leaving room for
per-flow isolated state when a game needs it (rollback / speculative
preview / forked branches).

The cost: more API surface than bladeink's single-global model. But
the request-component pattern + observer events keep the user-facing
spawn cost to about the same number of lines as bladeink, and the
ECS-natural shape pays off as soon as you have a second NPC.

### Request component over Commands extension

For "spawn a flow," we considered two patterns:

- **Commands ext**: `commands.brink_spawn_flow::<M>(handle, knot)` —
  imperative, hides state behind a function call.
- **Request component**: `commands.spawn(BrinkFlowRequest::<M>::builder().story(handle).build())`
  — declarative, the entity spawn IS the request, a system fulfills it
  reactively when assets are ready.

We chose the request-component pattern because:
- The entity is in a coherent state from spawn — it carries its own
  intent. No "spawn then later magically attach components."
- It naturally handles the "assets aren't loaded yet" case without
  polling or readiness latches in user code.
- It composes with hot-reload trivially: re-spawn a request, fulfillment
  rebuilds.
- It fits the ECS data-driven idiom; Commands extensions feel imperative
  in a system that's otherwise declarative.

`bon` was chosen for the builder because it handles generic structs
(including our `<M>` marker via `PhantomData`) cleanly.

### Observer events over MessageReader/Writer

Bevy 0.18 has two event flavors:
- **Messages** (`Message` derive, `MessageReader`/`MessageWriter`): the
  renamed buffered events from older Bevy versions. Per-tick polling.
- **Observer events** (`Event` derive, `commands.trigger`,
  `app.add_observer`): synchronous fire-and-react.

The user's standing position: "full on message reader/writer seems
unlikely to be useful" for this domain. Observer events match the
"a line was just produced" semantic better — it's a punctual event
addressed at a specific entity (the flow), not a stream consumed by a
poll loop. Splitting one big `BrinkLineMessage(Line)` into four
variant-specific events (`BrinkLineDelivered`, `BrinkChoicesPresented`,
`BrinkTurnDone`, `BrinkStoryEnded`) lets observers target exactly the
case they care about with no inline `match`.

(Earlier worry about `commands.trigger` reliability in some contexts
turned out to be a test-side bug, not a dispatch issue. See resolved
issue #2 in "Open issues" below for the post-mortem.)

### Init pass at load time, not per-flow

A story's top-of-file content is two things stuck together:
1. **VAR/CONST/LIST declarations** — link-time concern, handled by
   `program.global_defaults()`.
2. **Free-floating setup code** before any `=== knot ===` — runtime
   concern, runs as the start of player-visible content.

For a flow that starts at root, (2) is just the opening of the story
and runs naturally on advance. For a flow that starts at a *named knot*
(say, an NPC's dialogue), the player skips (2) entirely — but those
side effects (e.g. `~ initialize_save_data()`) still need to have
happened, or the named-knot flow runs against uninitialized globals.

The fix: run a one-time init pass at load time that captures the
post-(2) `Context` as a labeled `InitialGlobalsAsset`. Named-knot
flows seed `BrinkGlobals` from this snapshot; root-start flows seed
from fresh defaults so they don't double-execute init code.

Configurable via `InkLoaderSettings::run_init` (default `true`) for
stories whose init code calls host-provided externals not yet
registered at load time.

### CRC-aware asset modification — punted

The original sketch had separate `ProgramAsset` and `LineTablesAsset`
fire `Modified` events independently based on content CRC: edit a
string literal, only `LineTablesAsset` would re-emit; in-flight story
state preserved. The user explicitly punted: "if we don't have robust
support for independent hot-reloading of one versus the others, that's
okay."

So v1 always re-emits both subassets together when the source file
changes. Both `ProgramAsset` and `LineTablesAsset` exist as distinct
types (good for future split + locale overlays) but we don't try to
suppress one of them.

### `continue_maximally` is a bad primitive (parked)

The original `Story::continue_maximally` walks until any non-Text line
appears. The user's standing concern: external function calls can fire
earlier than expected during this walk, which is bad for game-side
effects that care about precise timing. The "real" continuation API
should be one-line-at-a-time (`continue()` style), with the consumer
deciding when to keep going.

Currently `BrinkFlow::advance_until_terminal` is the
`continue_maximally` shape. It's the right primitive for a "click to
continue" demo dialogue UI but probably wrong for a production
external-function-heavy game. Marked for redesign once external bindings
land.

### Fork mode for per-flow context (seam, no API) — superseded by F6.2

**Resolved differently than sketched here** — see the F6.2 note at the top
of this document. Per-entity privacy did *not* end up being "per-flow
`Context` on a Component instead of a shared Resource" (that would have
meant re-inventing full-clone isolation with a manual merge step, which is
exactly what F6.2 removed). Instead: `BrinkGlobals<M>` stays the one
shared `World` for every flow; a `WorldPolicy` installed once at plugin
setup marks *which* variables/knots are `Local` (private per flow, via
each flow's own `FlowLocal`) versus `World` (shared, the default).
Speculative/sandboxed evaluation (the runtime's `Mode::Sandbox` on
`FlowLocal`) is a separate, later mechanism for the "watch/eval" case —
see `docs/scoped-flow-state-spec.md`'s "Sandbox mode" section — not
something bevy-brink wires up yet. The rest of this section is kept
verbatim as historical record of the design that was rejected.

The shared-globals default (`BrinkGlobals<M>` resource) suits ~95% of
games. For the remaining 5% — speculative dialogue preview, rollback,
isolated side-conversations — a fork mode is needed: per-flow `Context`
on a Component instead of a shared Resource.

The runtime API supports this: the step functions take `&mut Context`,
not "the resource." The seam is intentional. We haven't built the
fork-mode wiring (`BrinkFlowRequest::isolated_context: bool`,
fulfillment branching, advance system handling per-entity context) but
the design space is clear and the cost is bounded.

The user explicitly said merge semantics (additive vs last-write-wins
vs takes-max) should NOT ship as a built-in — they're game-specific and
any default would be wrong for half the consumers. Fork mode would just
clone the `Context`; reconciliation back to shared state is consumer
policy.

### Inspiration: `bevy_bladeink`

`~/code/rs/bevy_bladeink/bevy_bladeink/examples/basic.rs` was the
primary reference for the Bevy-side ergonomics. The visible
inspirations:

- Per-event-variant observers (their `DeliverLine` / `DeliverChoices`).
- Single `InkStory` resource as the "bring up the story" verb (we kept
  this as a request-component instead, but the "one line of user code
  to start" target was the same).
- `commands.ink_*` verb pattern for advance/choose (we deliberately
  chose request components instead, but the simplicity of that surface
  set the bar).

Where we differ:
- Multi-flow design (vs. their single-global-story).
- Marker generic for compile-time multi-story support.
- Three-asset bundle (vs. their single `InkStory` resource).
- Asset/hot-reload integration (their basic example doesn't demonstrate
  hot-reload; we attempted to make it work and the visual UX is still
  parked — see issue #1).

## How to pick this work up cleanly

1. **Read this doc.** It is the canonical state.
2. **Run `cargo run --example play_story` from the repo root** to confirm
   the basic loading path still works.
3. **Decide what to tackle**. Most pressing:
   - (a) Diagnose and fix the `commands.trigger` issue — unblocks both
     the example's hot-reload and the failing flow tests.
   - (b) If (a) is hard, sidestep it by reverting hot-reload to "reset
     only" and switching event dispatch to `&mut World` + `world.trigger`.
   - (c) Move on to `EXTERNAL` function bindings (probably the next
     blocker for any real game).
4. **Test-first when adding features.** The fulfillment tests show the
   pattern; reuse `test_support` helpers.

## Recent commit history

```
dfb548d7 fix(bevy-brink): silent replay + post-replay advance fires real-state events
df0fa162 fix(bevy-brink): emit BrinkFlowReset before replay so UI clears at right time
db66d021 refactor(bevy-brink): observer events for line/choice delivery
8149075a feat(bevy-brink): dev-mode replay log for hot-reload UX
8d5ead69 refactor(bevy-brink): example uses BrinkFlowRequest pattern
8364b201 feat(bevy-brink): BrinkFlowRequest + fulfillment system
29619112 feat(bevy-brink): InkLoaderSettings + InitialGlobalsAsset
9733712d feat(runtime): Program::find_address for knot-name lookup
b9b6b38f feat(bevy-brink): visual play_story example with hot-reload
014a5a18 feat(bevy-brink): .ink source loader with INCLUDE-graph hot-reload
905e436a refactor(bevy-brink): split story asset into Program + LineTables + bundle
c5fb5691 feat(bevy-brink): add ProgramAsset and .inkb AssetLoader
b5ce52fa feat: scaffold bevy-brink crate with marker-parameterized types
26106783 refactor: swap Rc to Arc in Value and call-stack snapshots
45be24a2 feat: expose runtime primitives for external orchestration
```
