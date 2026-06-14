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

## 3. Data model (`brink-format`)

Lives in `brink-format` (serializable, alongside `Value` and `SaveState`), keeping
`brink-runtime` serde-free. Serializability lets consumers persist recordings (e.g. the web's
localStorage) and cross the wasm boundary if needed.

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

### 4.1 Recording (during the live run)

The stepping path takes an **optional** `&mut ReplayRecorder`. Every time an external's result
value is determined, it is appended: both the inline `ExternalResult::Resolved(v)` path
(pure/command bindings) and the out-of-band `resolve_external(v)` path (world-access/async
bindings). A single internal record site per path; `None` recorder = no-op = today's behavior.

### 4.2 Replay (`ReplayHandler`)

A `ReplayHandler` implementing `ExternalFnHandler`, parameterized by a `&mut ReplayRecorder`
and a `ReplayMode`:

```text
call(name, args):
  Live      → Fallback        (consumer's driver re-runs it live; see §6)
  Recorded  → match recorder.take_recorded(name, args):
                Some(v) → Resolved(v)
                None    → Fallback   (uncovered/divergent/past-cap → ink fallback body)
```

`Live` returns `Fallback` for unbound externals; for bound ones the consumer supplies its real
handler instead of (or composed with) the `ReplayHandler` — `Live` mode is "use your normal
handler," so in practice a consumer in `Live` mode just doesn't wrap with `ReplayHandler` at
all. The shared `ReplayHandler` is the `Recorded`-mode object.

No new async machinery: a `Live`-mode query that needs world access flows through the existing
`ExternalResult::Pending` / `resolve_external` suspend-resume (Track A); the consumer's
existing driver resolves it.

## 5. Resolved design questions

1. **Recording choke point** — at the two value-determination sites in the runtime stepping:
   the inline `Resolved` return and `resolve_external`. The recorder is threaded as an optional
   parameter through the stepping entry points (or carried on the stepping context), so a
   `None` recorder is a true no-op.
2. **Cap + divergence** — the replay cursor consumes recordings **strictly in order**, matching
   **name + args** at the current position. A match returns the recorded value and advances the
   cursor; the **first** mismatch (program/path changed) or exhaustion sets `diverged` and
   *all* subsequent lookups return `None` → fallback. This degrades gracefully when an edit
   changes the early path, rather than feeding misaligned later recordings. Cap = 16 384.
3. **Web serialization** — `RecordedExternal`/`ReplayRecorder` are `serde`-serializable (they
   live in `brink-format`). The web consumer keeps the recorder **Rust-side in the wasm
   `StoryRunner`** by default (no boundary crossing); persisting it alongside the choice log in
   localStorage is an additive option later.
4. **`Live` granularity** — whole-flow (one `ReplayMode` per replay), with a per-flow override
   at the consumer layer. No per-external granularity; revisit only if a real need appears.

## 6. Consumer adapters (out of scope for this issue's core, listed for context)

- **Bevy** (#173): record into a flow-attached `ReplayRecorder` during normal play; drive the
  exclusive `&mut World` reload replay with the shared `ReplayHandler`, resolving `Live`/world
  queries via `run_system_with`.
- **Web/studio**: record inside the wasm `StoryRunner`; `session.ts`'s choice-replay drives the
  shared handler over the wasm boundary. Closes the latent gap with no manifest `@kind`.
- **RMMZ** (celeris #78): align the host-side implementation onto this model.

## 7. Out of scope

Per-external mode granularity; recording anything other than external results (visit counts,
RNG, etc. are already part of the `Context`/`SaveState` snapshot); the consumer adapters
themselves (#173 and the web/RMMZ work).
