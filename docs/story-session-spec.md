# Story Session — implementation spec (#370 + #371 snapshots)

Status: **approved design** (2026-07-05 rulings; see `docs/design/story-session-api-proposals.md`
for the full design round and `docs/decision-log.md` "Story Session (#370): the journal is
Rust-canonical"). This spec is the build contract for Wave C of editor round 2.

## Architecture ruling (the big one)

**The journal is Rust-canonical.** The session layer lives in `brink-runtime` as a module that
*wraps* `FlowInstance` (composition, not threading — the VM never learns about journaling).
The existing `ReplayRecorder` generalizes into the full **session journal**. Consumers:

- **`bevy-brink`** — first-class: Bevy games get sessions/replay/save-load natively.
- **`crates/brink-web`** — wasm bindings exposing `StorySession` on `@brink-lang/web`.
- **`@brink/studio-store`** — `LocalSessionProvider` migrates onto the public session
  (its `SessionProvider` contract toward the store stays intact).

There is **no JS-side journal**. The web `StorySession` is a thin binding; the journal
serializes to JSON via serde (that JSON is what celeris persists in save slots).

**Hard constraint:** the journal layer is observation-only around the VM. Episode behavior is
untouched — the oracle corpus must not move. If any change would alter VM stepping, stop and
surface it.

## The journal (Rust, serde-serializable)

One ordered log of every input that entered the VM:

```rust
pub struct SessionJournal {
    pub version: u32,                    // 1
    pub program_checksum: String,
    pub seed: Option<u64>,
    pub events: Vec<JournalEvent>,
    pub truncated: bool,                 // cap or divergence truncation happened
    /// Fast-restore: terminal state snapshot (ruling: embedded SaveState in v1).
    pub checkpoint: Option<SaveState>,
}

pub enum JournalEvent {
    Start { path: Option<String>, args: Vec<Value> },   // play-from-here
    Choice { index: u32, label: Option<String> },
    External { name: String, args: Vec<Value>, result: Value },
    SetVar { name: String, value: Value },
    GoToPath { path: String, args: Vec<Value> },
    LoadState { state: SaveState },
    Call { name: String, args: Vec<Value> },            // journaled callFunction (ruling)
    // Reserved dimensions (serialize, don't interpret in v1):
    // - per-event `anchor: Option<u64>` position ordinal (ruling: turn-boundary
    //   contract in v1, anchors additive later)
    // - per-event `flow: Option<String>` tag (ruling: reserved; v1 is default-flow-only)
}
```

- Values serialize **tagged** (the `SaveState.globals` precedent) — `value_to_js`-style lossy
  mapping (List/Divert → null) is forbidden in the journal.
- `SESSION_JOURNAL_CAP` mirrors `RECORDING_CAP` (unbounded-growth rule). Hitting the cap sets
  `truncated` and the journal degrades honestly (restore falls back to `checkpoint`).
- Mid-turn mutation contract (ruling): `set_var`/`go_to_path`/`load_state` are **turn-boundary
  only** — the session queues them to the next pause (or errors, one behavior, documented).
  The schema reserves the anchor field so exact mid-turn replay can arrive additively.

## The session (Rust)

```rust
pub struct StorySession { /* owns FlowInstance + journal + replay cursor */ }

pub enum ReplayOutcome {
    Replayed { warnings: Vec<ReplayWarning> },       // label drift = soft warning (ruling)
    Diverged { at_event: usize, expected: JournalEvent, found: DivergenceFound },
    Failed   { at_event: usize, reason: FailReason }, // runtime-error | budget | awaiting-external
}
```

- **Stepping** mirrors `FlowInstance::advance()` exactly — `StepOutcome::{Line, AwaitingExternal}`.
  The session records external results where the VM receives them, which puts the
  **journaling-window gate** in the right place for free: `call_function` (isolated handler) and
  foreign flows do not write into the journal.
- **Replay** consumes a journal prefix against a (possibly recompiled) program. Divergence is a
  typed result, never silent, never thrown: truncate the journal at the divergence point, park at
  the reached position (the studio `replayWalk` semantics, now as data). The bail-to-fresh
  backstop is representable (`Failed { reason: Budget }` + caller restarts).
- **Recorded vs live externals**: recorded (journal-served) is the default; live re-invocation is
  an explicit option. Live replay hitting an async/deferred external parks
  (`AwaitingExternal`) and resumes via `continue_replay()` (ruling).
- **Snapshots are session methods** (ruling): `snapshot() -> StateSnapshot` (typed globals incl.
  list membership, turn counts, callstack — a NEW typed serialization path, not the string-valued
  `DebugState`) and a pure `diff(&StateSnapshot, &StateSnapshot) -> StateDiff`
  (added/removed/changed globals, list deltas, pushed/popped frames).
- **Fast-restore** (ruling: embedded checkpoint in v1): `restore(bytes, journal)` applies
  `checkpoint` and skips replay when checksums match; falls back to replay otherwise.
- **Escape hatch** (ruling): the wrapped runner/flow is reachable with a documented
  journal-bypass contract (shared flows #200 keep working; their externals never journal).

## Web binding (`crates/brink-web` → `@brink-lang/web`)

`StorySession` (TS) wraps the Rust session via wasm:

- `advance()`, `choose(i)`, `resolveExternal(v)`, `continueSingle()`, `continueToPause()`,
  `setVar`/`goToPath`/`saveState`/`loadState` (journaled), `callFunction` (journaled `Call`),
  `snapshot()`/`diff(a,b)` (+ standalone `diffSnapshots` export),
  `exportJournal(): SessionJournalJson` / `StorySession.restore(bytes, journal)`,
  `reload(bytes)` (recompile-in-place + replay → `ReplayOutcome`), `restart()`, `free()`.
- Fixes the wire-format lie: `awaiting_external` comes OUT of the `Line` union and into
  `StepOutcome` (`{ type: "line" } | { type: "awaiting_external" }`). The two park states are
  distinct: promise-in-flight (session awaits internally, never surfaced as resolvable) vs
  deferred out-of-band (host must `resolveExternal`) — graft the P2 `deferred: string[]` option.
- Shape hygiene: `type` + snake_case discriminants (match the existing `Line` union); no
  camelCase twin types.
- Wasm exports needed (the design round's honest accounting): `pending_external_args`,
  function-eval with recorder isolation, journal export/import, typed-snapshot serialization,
  recompile-preserving replay. Budget for real Rust work.
- Persistence hook: journal-append notification must be **deferred + debounced** (never
  synchronous under the wasm call / BusyGuard).

## Migration

- `LocalSessionProvider` (studio-store) reimplements on `StorySession`; its provider contract
  is unchanged. The persisted `{choiceLog}` localStorage blob gets a one-time migration
  (choice log → journal events) or a documented reset.
- bevy-brink exposes the session on its flow components (design detail owned by that crate; the
  Rust session API is the contract).

## Deliverables (Wave C build order)

1. **`brink-runtime` session module** — journal + session + replay + typed snapshot/diff + tests
   (including: divergence truncation, recorded-vs-live, callFunction isolation-but-journaled,
   checkpoint fast-restore, cap behavior). Oracle ratchet must not move.
2. **`brink-web` bindings + `@brink-lang/web` types** — StepOutcome split, session surface,
   JSON journal, tests (wasm-surface, multi-byte offsets where relevant).
3. **Studio migration** — `LocalSessionProvider` on the public session; studio e2e stays green.
4. **Docs** — consumer-guide section + changesets (`@brink-lang/web` minor).

Each deliverable is one PR on the merge train. #371's snapshot half ships inside (1)+(2).
