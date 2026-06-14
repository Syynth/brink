# Replay recording — shared external recording/replay primitive

**Status:** Design → implementation (issue #189). Cross-consumer foundation for faithful
hot-reload replay. The Bevy (#173), web/studio, and RMMZ (celeris #78) replay paths become
thin adapters over this.

> Tooling-only: this is additive. It does not change VM semantics, the compiled program, or
> the oracle. The runtime gains an *optional* recorder it writes to while stepping, and a
> replay-time `ExternalFnHandler`. With neither engaged, behavior is exactly as today.

## 1. Problem

Hot-reload replay reconstructs a flow by re-walking the program through the recorded choice
log. During that walk, external calls resolve through whatever handler is supplied — typically
the **fallback** handler. So a branch keyed on a side-effect-free read
(`{get_switch(1): A|B}`) takes whatever the *fallback body* returns, not the real value → the
walk diverges → only approximate position is restored. And if real externals *are* bound,
effect externals (`give_item`, `play_se`) re-fire → duplicated side effects.

## 2. Design — record every external, replay the recordings

The key decision (#189): **do not distinguish external kinds.** Rather than re-running the
program live during replay and deciding per-external what to do (which would need a
pure/query/effect classifier), **record the result of every external during the live run, and
on replay feed those recordings back.**

Because replay then **re-executes nothing**:

- **Effects don't double-fire** — a recorded `give_item` returns its recorded value instead of
  running. It is stubbed *by virtue of* being replayed, with no need to identify it as an effect.
- **Reads stay faithful** — the recorded value feeds the same branch decision.
- **Pure is moot** — replaying a recorded value equals recomputing it.

No `ExternalKind`, no classifier, and — notably — no dependency on the binding registry or the
host-capability manifest's `@kind` for replay. Recording is uniform, so it can be a single
**runtime-owned** concern rather than per-consumer hook sites.

## 3. Data model (`brink-runtime`)

Lives in **`brink-runtime`** (in `replay.rs`, next to the handlers). Recordings are **ephemeral
in-memory hot-reload state, not serialized** — the **transcript** is the durable artifact, so a
recorder never needs to persist. This keeps the types out of `brink-format` (whose charter is
the compiler↔runtime *interface* — a recording is a pure runtime-execution artifact the
compiler never produces) and keeps the runtime serde-free. They are plain data, so a consumer
that ever needs persistence can add serialization without disturbing this core.

```rust
/// One recorded external result, captured in call order during a live run.
pub struct RecordedExternal {
    pub name: String,
    pub args: Vec<Value>,
    pub result: Value,
}

/// An append-only, capped log of external results for one flow, with a replay cursor.
pub struct ReplayRecorder {
    log: Vec<RecordedExternal>,
    cursor: usize,     // replay position
    diverged: bool,    // set once a replay lookup mismatches; thereafter all → fallback
}

/// How a replay obtains external values. Whole-flow granularity.
pub enum ReplayMode {
    /// Default. Return the recorded result if the next entry matches (name + args),
    /// else the ink fallback body. Re-executes nothing.
    Recorded,
    /// Ignore recordings; run every external live (effects fire). The explicit
    /// "re-run against the current world" escape hatch.
    Live,
}
```

`ReplayRecorder` API (sketch): `record(name, args, result)` (append, respects the cap);
`take_recorded(name, args) -> Option<Value>` (the replay cursor lookup, §5); `reset_cursor()`;
`len`/`is_empty`. Cap: `RECORDING_CAP = 16_384` entries (unbounded-growth guard); beyond it,
`record` drops and replay falls through to fallback for the uncovered tail.

## 4. Runtime behavior (`brink-runtime`)

### 4.1 Recording (during the live run) — by composition, not threading

Recording **composes** with the consumer's handler rather than threading an optional recorder
through the stepping hot loop — which aligns with the project's instrumentation principle
("observers should wrap or compose with production types, not thread optional parameters
through them"). No stepping signature changes; not recording = don't wrap = exactly today's
behavior.

- **`RecordingHandler<H>`** wraps the real `ExternalFnHandler` and records every inline
  `ExternalResult::Resolved(v)` — the pure/command bindings that resolve while the VM steps.
  The consumer wraps its handler with this during a recording run.
- **Out-of-band results** — world-access/async bindings return `Pending`; their value arrives
  later via `resolve_external`. The consumer records those itself at the moment it supplies the
  value (it has the name, args, and result there). One line in each consumer's resolve path.

### 4.2 Replay (`ReplayHandler`)

`ReplayHandler` implements `ExternalFnHandler` over a `&mut ReplayRecorder` — it *is* the
`Recorded`-mode object (it resets the cursor on construction):

```text
call(name, args) → match recorder.take_recorded(name, args):
                     Some(v) → Resolved(v)
                     None    → Fallback   (uncovered/divergent/past-cap → ink fallback body)
```

For **`ReplayMode::Live`**, the consumer simply **doesn't** wrap with `ReplayHandler` — it
supplies its real handler so everything runs live (effects fire). `ReplayMode` is just the
consumer's config; the runtime handler itself is the recorded-replay object, so there is no
mode branch in the hot path.

No new async machinery: a `Live`-mode query that needs world access flows through the existing
`ExternalResult::Pending` / `resolve_external` suspend-resume (Track A); the consumer's existing
driver resolves it.

## 5. Resolved design questions

1. **Recording choke point** — composition, not threading. `RecordingHandler` (a wrapping
   `ExternalFnHandler`) captures inline `Resolved` results; the consumer records out-of-band
   (`resolve_external`) results when it resolves them. No optional parameter through the stepping
   hot loop — aligns with the project's "observers wrap/compose, not thread" principle.
2. **Cap + divergence** — the replay cursor consumes recordings **strictly in order**, matching
   **name + args** at the current position. A match returns the recorded value and advances the
   cursor; the **first** mismatch (program/path changed) or exhaustion sets `diverged` and
   *all* subsequent lookups return `None` → fallback. This degrades gracefully when an edit
   changes the early path, rather than feeding misaligned later recordings. Cap = 16 384.
3. **Persistence** — recordings are **not serialized**; they're ephemeral in-memory hot-reload
   state, and the **transcript** is the durable artifact. The web consumer keeps the recorder
   Rust-side in the wasm `StoryRunner`. If a consumer ever genuinely needs to persist a
   recording, the types are plain data and can grow serialization then — no `serde` in this core.
4. **`Live` granularity** — whole-flow (one `ReplayMode` per replay), with a per-flow override
   at the consumer layer. No per-external granularity; revisit only if a real need appears.

## 6. Consumer adapters (out of scope for this issue's core, listed for context)

- **Bevy** (#173): record into a flow-attached `ReplayRecorder` during normal play; drive the
  exclusive `&mut World` reload replay with the shared `ReplayHandler`, resolving `Live`/world
  queries via `run_system_with`.
  - **Landed (Recorded mode):** recording now covers **both** playback paths and all external
    kinds, into the entity's dev-only `BrinkReplayLog` recorder:
    - The exclusive `advance_flow` driver records every external it resolves — inline pure/command
      via `RecordingHandler`, out-of-band world-access queries at the resolve site (recorder taken
      out/put back around the pass).
    - The non-exclusive path records via `BrinkFlow::step_one_recording` /
      `advance_until_terminal_recording` (inline pure/command, mirroring `choose_recording`) plus
      the plugin resolver's out-of-band sites: `dispatch_one_external` (query),
      `resolve_external_world` (async event), and `poll_brink_tasks` (task). Out-of-band recording
      is unconditional for any dev-tracked flow — a *partial* recording (e.g. a flow driven by
      plain `step_one`) just diverges to fallback earlier on replay, never worse than recording
      nothing.

    `replay_on_reload` drives the whole re-walk through one `ReplayHandler` over the recorder, so a
    query-gated branch replays the value it took during play, recorded commands don't re-fire, and
    a recorded async/task value replays **synchronously** (the handler serves it inline, so no task
    is spawned and no event fires). Uncovered/divergent calls fall through to the ink fallback body.
  - **Deferred:** `ReplayMode::Live` — the `BrinkReplayConfig`/`ReplayQueryModeOverride` seam
    exists but isn't consumed yet; it needs the world-re-query refactor with the effect-re-fire
    caveat. Recording is otherwise complete for the Bevy adapter.
- **Web/studio**: record inside the wasm `StoryRunner`; `session.ts`'s choice-replay drives the
  shared handler over the wasm boundary. Closes the latent gap with no manifest `@kind`.
- **RMMZ** (celeris #78): align the host-side implementation onto this model.

## 7. Out of scope

Per-external mode granularity; recording anything other than external results (visit counts,
RNG, etc. are already part of the `Context`/`SaveState` snapshot); the consumer adapters
themselves (#173 and the web/RMMZ work).
