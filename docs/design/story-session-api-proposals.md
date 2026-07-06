# Issue #370 — Public Story Session Primitive: Design-Round Comparison

**For:** project owner rulings · **Scope:** `@brink-lang/web` public API · **Date:** 2026-07-05

---

## 1. The design problem

Celeris' #937 boundary ruling sends the session/replay seam public: the studio's private `LocalSessionProvider` (choice-log replay-on-recompile, ~590 lines in `packages/studio-store/src/session/local-provider.ts`) is the reference material, and #370 asks for its public API on `@brink-lang/web`.

**Requirements from the first consumer (celeris Player):**
- replay-on-recompile mid-edit (celeris' player currently cannot replay mid-edit),
- first-class `awaiting_external` handling (currently ignored host-side),
- the seam for the runtime-inspection cluster: single-line stepping (celeris#912), full state snapshot surfacing (celeris#913), save/edit/load story states (celeris#914).

**Locked rulings all three proposals designed against:**
1. Replay divergence is a first-class result — never silent, never thrown.
2. Both external-replay modes (recorded, live) must be representable.
3. Snapshot/diff (#371) are session methods.
4. Session owns choice log + replay + external-fn pending state + snapshots; hosts own player-UI state (transcript rendering, persistence storage, notifications, reactivity).
5. The studio's `LocalSessionProvider` is the first migration consumer; its `SessionProvider` contract toward the store stays intact.

**Notable convergence:** all three proposals independently arrived at the same skeleton — a `StorySession` class on `@brink-lang/web` wrapping `StoryRunnerHandle`; a stepping surface mirroring the Rust `FlowInstance::advance() → StepOutcome::{Line, AwaitingExternal}` seam (fixing the current wire-format lie where `awaiting_external` is smuggled into the `Line` union); divergence as a typed three-way report generalizing the proven `replayWalk` (truncate-at-divergence, park at reached position); recorded externals as the default replay mode with `"live"` as escape hatch; typed snapshot + semantic diff. The proposals differ on **what the durable artifact is**, **how much the session journals**, and **how honestly they account for the Rust/wasm work required**. The critiques found that every proposal understates the wasm scope, and that the awaiting-external protocol and record/replay mechanics need real repair in all three.

---

## 2. The proposals

### Proposal 1 — Minimal mirror of the FlowInstance seam

**Thesis:** publish the thinnest faithful TS projection of the Rust `FlowInstance` seam; log/replay/snapshot are a pull-based wrapper layer (Layer 2) implemented purely in terms of the mirrored seam (Layer 1) plus the existing Rust `ReplayRecorder`. The choice log lives TS-side; recorded externals stay in the Rust recorder. Claimed Rust cost: three small exports (`pending_external_args`, `begin_function_eval`/`resume_function_eval`, and deferrable `export_recording`/`import_recording`).

**API sketch (condensed):**

```ts
type StepOutcome =
  | { kind: "line"; line: Line }
  | { kind: "awaiting-external"; name: string; args: ExternalValue[] };

interface SessionLog { checksum: string | null; choices: ChoiceLogEntry[]; externals: ExternalLogEntry[]; }

type ReplayResult =
  | { status: "replayed"; lines: Line[]; log: SessionLog }
  | { status: "diverged"; atStep: number; expected: ChoiceLogEntry; found: DivergenceFound; lines: Line[]; log: SessionLog }  // log = truncated prefix
  | { status: "failed"; atStep: number; error: string; lines: Line[]; log: SessionLog };

class StorySession {
  constructor(bytes: Uint8Array, opts?: { seed?; startPath? });
  static adopt(runner: StoryRunnerHandle): StorySession;
  readonly runner: StoryRunnerHandle;                    // escape hatch: READ + BIND only by convention
  advance(): StepOutcome;
  resolveExternal(v: ExternalValue): void;
  beginFunctionEval(name, ...args): FunctionEvalOutcome;
  continueSingle(): Promise<Line>;  continueMaximally(): Promise<Line[]>;
  choose(index: number): void;                           // the only recording choice entry point
  replay(log: SessionLog, opts?): Promise<ReplayResult>; // runner.reset() first, then re-walk
  reloadAndReplay(bytes, opts?): Promise<ReplayResult>;  // the recompile primitive
  snapshot(): StateSnapshot;                             // typed debug_snapshot projection, string values
  diff(a, b): SnapshotDiff;                              // + standalone diffSnapshots()
}
```

**Keyed strengths (from critique):**
- The impedance-mismatch diagnosis is verified and real: brink-web's `advance_one` does smuggle `{ type: "awaiting_external" }` into `Line` while Rust deliberately keeps it out; restoring the separation is the right fix at the right layer.
- The divergence contract encodes the proven `replayWalk` semantics line-for-line as data (truncated prefix, park, choices/done/end kinds, budget) — a genuine improvement over side-effecting a store.
- The `"auto"` externals default is byte-for-byte the studio's current `hasRecording()` heuristic; recorded/live maps exactly onto the replay-recording spec.
- Layering discipline (Layer 2 built only on Layer 1) honors the instrumentation-composes principle; the `runner` escape hatch avoids re-wrapping ~40 existing methods.
- Scoping matches the decision log: transcript/persistence/notifications host-side, flows deferred.

**Keyed holes (from critique):**
- **Replay destroys its own input:** `replay()` calls `runner.reset()` first, but Rust `reset()` drops the `ReplayRecorder` — so recorded-mode replay can never serve anything and `reloadAndReplay` wipes the recording that `reload()` preserved. Needs a recorder-preserving rewind or reload-aware replay: the "three small wasm exports" claim is false.
- **The FlowInstance-mirror claim breaks at its own seam:** on web, `awaiting_external` means a bound JS callback already ran and returned a Promise — the celeris manual `advance()`/`resolveExternal` example double-fires the effect and abandons the in-flight Promise; `takePendingPromise` (the real resolution source) is missing from the surface.
- **Function-eval corrupts the replay log:** `resolveExternal` is shared between story-advance and function-eval and records into the strictly-ordered recorder; the new eval exports need the recorder isolation `call_function` already has.
- **celeris#914 not served with `export_recording` deferred:** after app restart a restored log replays either live (re-firing every effect) or recorded-with-empty-recorder (breaking query-gated branches). Export/import must be v1 or the consumer story is wrong.
- `choose()` records before knowing the choose succeeded (today's provider records after); `adopt()` yields a session whose empty log misrepresents history, so replay silently rewinds wrong.
- `ReplayResult` can't represent the existing bail-to-fresh backstop (budget exhausted → restart from scratch), making the "parked" contract a lie in that case.
- Shape drift inside one package: camelCase `StateSnapshot` duplicating snake_case `DebugState`; `kind`+kebab-case discriminants next to `type`+snake_case `Line`; no migration story for the studio's persisted `{choiceLog}` localStorage format.
- startPath/flows leak: `replay()` resets to program root so play-from-here sessions need unspecified re-entry; `reloadAndReplay` with live shared flows silently invalidates state the session doesn't know exists.

**Critique verdict:** adopt-with-changes — skeleton right, but not implementable as specced (recorder lifecycle, park protocol, eval isolation are disqualifying as written; all fixable within the shape).

---

### Proposal 2 — Consumer-first typed input log

**Thesis:** derive the surface by enumerating every call the celeris Player and `LocalSessionProvider` actually make. Widen the choice log to a **typed input log** — choices + `goto` + `set-var` + `load-state` — so play-from-here and save-editing histories are replayable instead of silently lost. A persisted `SessionRecord` (version 1, inputs + recorded externals) is the save-slot artifact; `StorySession.restore(bytes, record)` is the load path. Introduces `deferred: string[]` — external names that always park for out-of-band resolution (the FlowInstance world-query pattern, which the web seam otherwise lacks).

**API sketch (condensed):**

```ts
type SessionEvent =
  | { kind: "line"; line: Line } | { kind: "choice"; index; label }
  | { kind: "goto"; path; args } | { kind: "set-var"; name; value }
  | { kind: "load-state"; state: SaveState }
  | { kind: "external"; name; args; result };

interface SessionRecord { version: 1; programChecksum: string; seed: number | null;
                          inputs: SessionInput[]; externals: {...}[]; }

type ReplayReport =
  | { status: "replayed"; steps: number }
  | { status: "diverged"; step; expected: ReplayExpected; found: ReplayFound; applied: number }
  | { status: "failed"; step; reason: "runtime-error" | "budget-exhausted" | "awaiting-external"; error? };

class StorySession {
  constructor(bytes, opts?: { seed?; externals?; deferred?: string[]; lenientUnbound?; maxReplayPasses? });
  readonly status; readonly choices; readonly events; readonly programChecksum;
  advance(): AdvanceOutcome;  resolveExternal(v): void;
  continueSingle(): Promise<Line>;  continueToPause(): Promise<Line[]>;
  choose(i): void;  goToPath(path, ...args): void;                 // both logged
  getVar/setVar (logged) / callFunction (NOT logged);
  snapshot(): StateSnapshot;   // TYPED SnapshotValue incl. list membership + turns
  diff(a, b): StateDiff;       // added/removed/changed globals, list deltas, pushed/popped frames
  saveState()/loadState (logged);
  export(): SessionRecord;
  reload(bytes, opts?): Promise<ReplayReport>;                     // recompile-in-place + replay
  static restore(bytes, record, opts?): Promise<{ session; replay: ReplayReport }>;
  restart(): void;  free(): void;
}
```

**Keyed strengths (from critique):**
- The consumer-call inventory is genuinely grounded — every wasm method it builds on exists as claimed, and its claimed gaps all verify (`pending_external_args` Rust-side but not exported; recorder never crosses the boundary; `DebugGlobal.value` is a pre-rendered string making semantic diff impossible today).
- Divergence-as-data faithfully generalizes `replayWalk` (truncate, park, budget, bail-to-fresh), and the reload-throws-only-on-decode/link claim matches the wasm implementation exactly.
- `AdvanceOutcome` mirrors the runtime seam with `continueSingle`/`continueToPause` layered exactly as `step_single_line` layers over `advance` — no competing seam.
- No storage/emitter/transcript on the primitive matches the locked ownership ruling; both first consumers already wrap in their own reactive layer.
- Recorded-by-default matches `ReplayMode::Recorded` being `#[default]` in brink-runtime; the fresh-recording⇒effectively-live asymmetry is honestly preserved.
- Widening to a typed input log addresses a real, verifiable gap: goto/set-var/load-state genuinely are not replayable today, so play-from-here and save-editing histories would silently vanish under a bare `number[]`.
- Its open questions (callFunction fidelity, flow dimension in the record, label drift) are the right forks for design review.

**Keyed holes (from critique):**
- **`SessionRecord.externals` cannot round-trip the recorder:** the Rust recorder logs `brink_format::Value` and matches on exact equality, but `value_to_js` maps List/Divert to null — any list-valued arg/result round-trips as Null, mismatches, latches diverged, and silently degrades everything after to fallback. The record needs tagged Values (the `SaveState.globals` precedent).
- **Incoherent truncation:** the input log truncates at `applied` on divergence but no wasm API truncates the recorder — after one divergence the persisted externals contain a stale tail that poisons every future restore.
- **Non-choice inputs are position-less:** replay consumes choice-to-choice passes, so a `set-var` issued mid-turn re-applies at the pause boundary — conditional text/choices between evaluate differently, producing phantom divergence with no story edit. Needs line-count anchors or advance-granularity re-application; neither specified.
- **"Two small wasm additions" is false:** the typed snapshot (typed globals, list membership, turns by path) is a third, substantially larger Rust export — `DebugState` has string globals, no turns table, and turns in `SaveState` are hash-keyed.
- **`advance()` conflates two park states** (async-binding-Promise-in-flight vs deferred-out-of-band) and drops `takePendingPromise`, creating a double-resolution race; and the deferred park is unobservable through `continueSingle` — the proposal's own celeris awaiting-external snippet is dead code while the await is suspended.
- **Replay-only restoration doesn't scale to shipped-game saves** (celeris#914's core case): O(full history) on every load, uncapped events log (violating the unbounded-growth rule), and past `RECORDING_CAP` the external tail structurally diverges. Fast-restore must be answered before `version: 1` freezes.
- **The studio migration breaks #200 shared flows:** `spawnFlow` hands the same runner to `FlowSessionProvider`; the session exposes no flow API and no raw-handle hatch — a regression, not a scoping decision.
- `ReplayExpected` has no variants for set-var/load-state divergence (undeclared global, `LoadReport` with unknowns) — the report vocabulary wasn't extended to match the log vocabulary; and the "append-only" log framing is contradicted by reload regenerating/truncating it, leaving `StateSnapshot.step` dangling across a diverged reload.

**Critique verdict:** adopt-with-changes — skeleton correct and well grounded, but the persistence/replay core is not freezable as specified (record format, truncation coherence, anchors, park protocol, report vocabulary, flow path, wasm scope all need repair first).

---

### Proposal 3 — Log-first (event-sourced) session

**Thesis:** the durable artifact is the **`SessionLog`** — an ordered, versioned, JSON journal of *every* input that entered the VM (start point, seed, choices, **external results**, var-sets, jumps), living JS-side. `StorySession` is a replayer over that log: save/load, replay-on-recompile, time-travel scrubbing, and divergence reporting all fall out of one mechanism (replay a log prefix against a program) instead of three (localStorage choiceLog + Rust `ReplayRecorder` + `SaveState`). Snapshots are stamped with a `logIndex`, making diff and time-travel two views of one timeline. Claims zero new wasm exports because every input already crosses the JS boundary.

**API sketch (condensed):**

```ts
type LogEvent =
  | { t: "start"; path?; args? } | { t: "seed"; seed }
  | { t: "choice"; index; label? }
  | { t: "external"; name; args; result }        // journaled at the binding-call / resolveExternal site
  | { t: "set_var"; name; value } | { t: "go_to_path"; path; args? };

interface SessionLog { version: 1; programChecksum: string; events: LogEvent[]; }
const SESSION_LOG_CAP = 16_384;                  // mirrors RECORDING_CAP

interface ReplayReport {
  outcome: { status: "replayed" } | { status: "diverged"; atEvent; expected: LogEvent; found: DivergenceFound }
          | { status: "failed"; atEvent; error };
  lines: Line[];                                  // regenerated output — host rebuilds transcript
  log: SessionLog;                                // consumed prefix on divergence
  pending: { name; args } | null;                 // live-mode async park
}

class StorySession {
  static start(bytes, opts?): StorySession;
  static resume(bytes, log, opts?): { session; report: ReplayReport };   // THE load path; journal-served externals
  recompile(bytes, opts?): ReplayReport;          // in-place reload; Rust recorder serves the re-walk
  continueReplay(): ReplayReport;                 // resume a live-mode replay parked on an async external
  advance(): SessionStep;  advanceToTurnEnd(): Promise<Line[]>;
  choose(i); resolveExternal(v);                  // both journaled
  setVar / goToPath / bindExternal (wrapped so sync results journal);
  get log(); exportLog(); onLogAppended(listener);  // persistence hook, no polling
  replayTo(logIndex): ReplayReport;               // time-travel scrubber (truncates + replays prefix)
  snapshot(): StateSnapshot;                      // typed globals/lists/turns, stamped logIndex
  static diff(a, b): StateDiff;                   // pure, usable on retained snapshots
  get runner(): StoryRunnerHandle;                // documented journal-bypassing hatch (flows, save/load)
}
```

**Keyed strengths (from critique):**
- The stepping surface genuinely mirrors the existing seam with no competition; awaiting-external as first-class data is exactly #370's stated requirement.
- Divergence semantics map 1:1 onto the proven `replayWalk`, and structured expected/found is a real upgrade over the bare `REPLAY_DIVERGED_MESSAGE` string.
- Recorded-as-default is correctly grounded in the shipped `ReplayMode::Recorded` and the spec's no-classifier decision.
- **Log-first fills a verified gap:** `SaveState` explicitly captures no execution position, and recorder durability was explicitly deferred by the replay-recording spec — a JS-side durable journal is a coherent answer that genuinely delivers full-page-reload durability, which upgrades the studio's persisted artifact for free.
- Snapshot/diff as session methods honors the locked ruling; pure static `diff` over value objects deletes the second diff implementation celeris#913 would otherwise write.
- Unbounded-growth discipline carried through (`SESSION_LOG_CAP`, budget guard); the seed story is sound (default rng seed is deterministic).
- The migration keeps the `SessionProvider` contract untouched — blast radius genuinely stops at provider internals.

**Keyed holes (from critique):**
- **The recompile path is incoherent as specced:** during `begin_replay` the Rust handler serves recorded values *without invoking JS bindings*, so the JS journal cursor never observes external calls in the recompile path — external divergence is undetectable there, and journal truncation is ill-defined when unobservable events interleave with visible choices. "Same outcome type either way" papers over materially different semantics. Either recompile replay must also be journal-served (retiring `begin_replay` for this path) or wasm must grow divergence/truncation accessors.
- **The two-recorder "drift is contained" claim is false both directions:** journal truncation has no Rust counterpart (recorders permanently disagree after first divergence), and `call_function` uses a non-recording handler while the wrapped JS bindings *would* journal its externals.
- **Journal pollution:** `continue_flow` (shared flows via the studio's `openFlow`) and `callFunction` invoke the same wrapped bindings — foreign external events append into the primary session's single-cursor log and corrupt every future replay. Needs an explicit journaling-window gate (journal only inside the session's own advance/choose/resolveExternal frames); absent from the design.
- **No mid-turn anchors:** the same defect as Proposal 2 — `setVar` between two `advance()` calls replays at a different VM position; per-step snapshots within a turn all alias to the same `logIndex`, so "jump to any captured moment" can't reproduce mid-turn moments.
- **`replayTo` is destructively one-way:** it truncates the journal, so every retained snapshot with a later `logIndex` dangles; clicking a later history row after an earlier one breaks. The scrubber needs a non-destructive form.
- **`snapshot()` is not constructible from the current wasm surface** ("wraps debug_snapshot + typed global reads" fails: string globals, `getVar` can't distinguish int/float or represent lists, no turns table, hash-keyed turns). The no-new-wasm claim quietly fails here.
- Missing journaled mutation surfaces: no `setSeed` (runner-hatch seeding silently breaks shuffle replay); `load()` only on the hatch; `lenientUnbound` changes resolution but isn't journaled.
- Contract gaps: `onLogAppended` fires synchronously inside a wasm call under `BusyGuard` (listener touching the runner throws reentrancy); per-append `JSON.stringify` persistence is O(n²) churn vs today's on-choose-only save; `truncated` field documented but absent from the schema; `resume()`'s decode-failure return shape unspecified; `ReplayReport.lines` alone can't reconstruct the studio's `"> chosen text"` markers.

**Critique verdict:** adopt-with-changes — "the right design to build 0.8.0 on" once four defects are fixed: (1) coherent recompile external-source story, (2) journaling-window gate, (3) position anchors + non-destructive `replayTo`, (4) honest typed-snapshot wasm export (plus the smaller contract gaps).

---

## 3. Comparison table

| Dimension | P1 — Minimal mirror | P2 — Typed input log | P3 — Event-sourced journal |
|---|---|---|---|
| **Durable artifact** | TS choice log + Rust recorder (export deferred to v2) | `SessionRecord` (inputs + externals), version 1 | `SessionLog` — full JS-side journal incl. externals |
| **Survives app restart / save slots (celeris#914)** | ✗ as specced (externals empty across restarts → live re-fire or fallback) | ✓ in intent; record format broken as written (lossy Values, stale recorder tail) | ✓ by construction (journal is JSON; no recorder serialization needed) |
| **Log scope** | Choices only (startPath in session, not log) | Choices + goto + set-var + load-state | Everything: start, seed, choices, externals, set-var, goto |
| **Replay-on-recompile mechanics** | `reloadAndReplay` — **broken**: `reset()` destroys the recorder it depends on | `reload()` — recorder/log truncation incoherent | `recompile()` — Rust-recorder path invisible to journal cursor; needs unification or wasm accessors |
| **Position anchoring of non-choice inputs** | N/A (choices only) | ✗ missing — phantom divergence | ✗ missing — same defect + logIndex aliasing |
| **Awaiting-external protocol** | Broken (double-fire example; no `takePendingPromise`) | Right idea (`deferred` names) but conflates the two park states; unobservable through `continueSingle` | Journaled resolution site is right; same park-state conflation risk |
| **Function-eval story** | Exports proposed but recorder-corruption unaddressed | `callFunction` unlogged — silent replay divergence risk | Open question; wrapped bindings would journal it inconsistently vs Rust |
| **Snapshot typing** | String values (ships today; weak diff) | Typed values + lists + turns (large unacknowledged wasm export) | Typed + logIndex stamp (same unacknowledged export) |
| **Time travel** | Not offered | Not offered | Offered; destructive/one-way as specced |
| **Persistence hook** | None (host polls exportLog) | None (host saves after own calls) | `onLogAppended` — reentrancy + O(n²) footguns as specced |
| **Shared flows (#200) under migration** | Untouched (silent invalidation risk documented as hole) | **Regression** — no flow API, no hatch | Survives via `runner` hatch, but pollutes the journal without a gate |
| **Escape hatch** | Full `runner`, convention-guarded | None (hole) | Full `runner`, documented journal-bypassing |
| **Honest wasm scope** | Understated (recorder-preserving rewind + eval isolation missing) | Understated (typed snapshot is a 3rd large export) | Understated ("zero exports" fails for snapshot; likely divergence accessors too) |
| **True Rust/wasm work (per critiques)** | Medium | Medium-large | Medium-large |
| **Critique verdict** | Adopt-with-changes; *not implementable as specced* | Adopt-with-changes; core not freezable yet | Adopt-with-changes; *"the right design to build 0.8.0 on"* after 4 fixes |

**What every critique settled (no ruling needed):**
- `StepOutcome`-style mirror replacing the fake `awaiting_external` Line variant — unanimous, verified correct.
- Divergence as a typed report with truncate-and-park semantics, plus the bail-to-fresh backstop representable in the result union.
- Recorded externals as default; live as explicit escape hatch; the fresh-recording⇒effectively-live asymmetry kept honest.
- The two park states (async-binding Promise in flight vs deferred out-of-band) must be distinguished; `takePendingPromise` or equivalent must be reachable/internalized.
- `choose()` records only after a successful choose.
- Function-eval and shared-flow binding invocations must be isolated from the replay record (recorder isolation Rust-side; journaling-window gate JS-side).
- Serialized external values must be tagged (SaveState.globals precedent), not raw `ExternalValue` (lists/diverts null out).
- Discriminant/casing conventions must be consistent with the package's existing `type` + snake_case `Line` union.
- The wasm surface is bigger than any proposal admits: `pending_external_args`, a typed-snapshot export, function-eval exports with recorder isolation, and either a recorder-preserving rewind or journal-served recompile.
- Persistence emission must be deferred (not synchronous under `BusyGuard`) and debounced.
- The studio's existing `{choiceLog}` localStorage format needs an explicit migration/compat note.

---

## 4. RECOMMENDATION

**Winning basis: Proposal 3 (log-first / event-sourced journal), rebuilt with the critique's four fixes and grafts below.**

Rationale:
- It is the only proposal whose durable artifact actually serves celeris#914 across process restarts without serializing the Rust recorder: P1 defers exactly the export its own consumer story requires; P2's record format is broken as written and still depends on a recorder it cannot truncate. P3's journal records external results at the JS boundary where they already cross, so durability is structural, not bolted on.
- One mechanism (replay a log prefix) subsumes what the others implement as three: save/load, replay-on-recompile, and time-travel. Fewer mechanisms = smaller frozen surface, which is #351's whole warning.
- Its critique verdict is the strongest of the three ("the right design to build 0.8.0 on"), and its defects are protocol repairs, not architecture replacements — whereas P1's headline primitive cannot work without Rust changes it denies needing, and P2's record format cannot round-trip.
- The deep problem the critiques exposed — **two recorders (Rust + JS) with incoherent truncation** — is resolved most cleanly inside P3's frame: make the journal the single source of truth and serve *all* replay (including recompile) from it, demoting `begin_replay`/`end_replay` to an internal optimization or retiring it for this path (subject to Decision 1 below).

**Graft from Proposal 2:**
- The **`deferred: string[]` externals option** — the only clean web-side realization of the FlowInstance "park for out-of-band resolution" pattern — but split the two park states explicitly: `awaiting-external (deferred)` (host must `resolveExternal`) vs promise-in-flight (session awaits internally; never surfaced through `advance()` as resolvable).
- The **closed `SessionInput` union + `version` field discipline** and the requirement that the **report vocabulary covers every input kind** (set-var against a deleted global, `LoadReport` with unknowns → representable divergence, not silent no-op).
- The **`static restore(bytes, record) → { session, replay }`** construction shape (P3's `resume` is equivalent; keep the pair-return).
- The typed `SnapshotValue`/`StateDiff` shape (list membership deltas, added/removed/changed globals) — with the honest admission that it requires a new Rust typed-snapshot export.

**Graft from Proposal 1:**
- The explicit **Layer 1 / Layer 2 discipline**: the log/replay layer implemented strictly in terms of the public stepping seam, so it cannot drift from VM semantics and is unit-testable without a browser.
- The **standalone pure `diffSnapshots()`** export (session method delegates), so hosts can diff persisted snapshots without a live session.
- The `"auto"` externals sensibility folded into documentation (fresh log ⇒ effectively live), rather than a third mode.
- Shape hygiene: reuse/align with existing `wasm-types` conventions (`type` + snake_case discriminants; don't ship a camelCase twin of `DebugState` without a deprecation plan).

**Mandatory repairs before freeze (settled by critiques, restated as build requirements):** journaling-window gate around session-owned frames; per-event position anchors (or a ruled restriction, Decision 3); non-destructive `replayTo`; deferred + debounced `onLogAppended`; `truncated` flag in the schema; tagged-value serialization for `external`/`set_var` events; journaled `setSeed`/`loadState`; specified decode-failure shape for `resume`; specified choice-line pairing so hosts can rebuild `"> choice"` transcripts from `ReplayReport.lines`; localStorage migration note for the studio.

---

## 5. Decisions for the project owner

Only genuine either-way calls remain — everything the critiques settled is listed above as build requirements.

1. **Recompile replay's external source — unify on the journal, or keep the dual path?**
   (a) *Unify:* `recompile()` serves externals from the JS journal like `resume()` does; `begin_replay`/`end_replay` is retired for this path. One divergence semantics everywhere; cost: in-page recompile loses the byte-identical Rust-recorder fidelity and the shipped Rust replay bracket becomes web-unused. (b) *Dual:* keep the Rust recorder for in-place recompile and add wasm accessors (`diverged()`, cursor position, truncate/compact-at-cursor, recorder-preserving rewind) so the JS report can be honest. More wasm surface, two mechanisms to keep coherent forever.

2. **Fast-restore for long/shipped-game saves (celeris#914) — what does `version: 1` commit to?**
   Replay-only restoration is O(playthrough) and collides with the log cap. Options: (a) reserve a `{ t: "checkpoint", state: SaveState }` event variant in the v1 schema now (forward-compatible, implement later); (b) allow the record to optionally embed a terminal `SaveState` for apply-state-skip-replay when checksums match; (c) ship replay-only and accept a format break at v2. The critiques agree this must be *answered* before the format freezes; they don't agree on which answer.

3. **Position anchoring of non-choice inputs.**
   (a) Anchor every input event with a line/step ordinal so replay re-applies at the exact VM position (heavier log, exact fidelity); or (b) restrict `set_var`/`go_to_path`/`load_state` to turn boundaries by contract (session throws or queues mid-turn) and document it. (b) is simpler and matches how the studio drives today; (a) is what celeris' engine-driven setVar pattern actually wants.

4. **Live-mode replay hitting a deferred/async external.**
   (a) Park with `ReplayReport.pending` + `continueReplay()` (P3 — resumable, but a stateful two-call protocol on the session); (b) accept an optional async resolver callback `(name, args) => Promise<ExternalValue>` on `resume`/`recompile` so live replay completes unattended (P2 open question); (c) fail with `reason: "awaiting-external"` and let the host re-drive manually. Pick one; supporting all three bloats the frozen surface.

5. **`callFunction` on a session: journaled, or documented-unsafe?**
   Journaling it (as a replayed `call` event) keeps replay deterministic even when an ink function mutates globals, at the cost of re-invoking functions on every replay and a Rust-side isolation story for its externals. Not journaling it (P2) is simpler but reintroduces exactly the "divergence with no logged cause" failure the input log exists to prevent.

6. **The escape hatch and shared flows (#200).**
   (a) Expose the full `StoryRunnerHandle` (P1/P3) with a documented journal-bypassing contract — preserves `spawnFlow` and everything not yet lifted, at known bypass risk; (b) a narrowed read/bind-only view plus explicit flow entry points on the session — enforced integrity, more frozen surface now. Related sub-call: should the v1 log schema reserve a flow-tag dimension on events, or is the session default-flow-only with flows explicitly "not replayed"?

7. **Typed snapshot: gate v1 on the new Rust export, or ship strings first?**
   Typed globals/lists/turns require a new Rust typed-snapshot serialization path (list-item name resolution, path-keyed turns — with hash-keyed unnamed scopes remaining unresolvable). (a) Build it now so `StateDiff` is semantic from day one (celeris#913's point); (b) ship the string-valued `DebugState` projection v1 (write_inkt precedent) and widen `SnapshotValue` later — faster to ship, but the diff is string-equality until v2 and the shape change is a soft break.

8. **Choice identity under edits: index-only, or index + label-drift warning?**
   Index-matching is today's semantics and the C# convention. Should a label mismatch at the same index (author edited the text of the choice the player "took") surface as a soft `warnings` field on a `"replayed"` result, or is that noise? Affects the report shape, so it must be decided before freeze.

9. **Where the primitive lives and what it's called.**
   `StorySession` in `@brink-lang/web` next to `StoryRunnerHandle` (all proposals) vs a separate `@brink-lang/session` package to keep web thin; and `StorySession` vs `StorySessionHandle` (the `EditorSessionHandle` precedent — is the `-Handle` suffix a convention worth keeping at this boundary or an artifact worth dropping?).