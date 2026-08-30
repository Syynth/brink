/**
 * Local session provider — the default `SessionProvider` backed by the
 * public `StorySessionHandle` (`@brink-lang/web`, docs/story-session-spec.md
 * deliverable 3, #388).
 *
 * Owns the session lifecycle, the persisted journal, silent replay-on-recompile
 * (via `reload()` → typed `ReplayOutcome`), and the pull-based stepping. Every
 * drive operation ends by recomputing the {@link SessionSnapshot} and notifying
 * subscribers; the store mirrors those snapshots into its reactive fields.
 * The `SessionProvider` contract toward the store is unchanged from the
 * `StoryRunnerHandle`-backed predecessor — the seam is provider-internal.
 *
 * ## Migration from the pre-#388 provider
 *
 * The old provider drove a `StoryRunnerHandle` directly and persisted a bare
 * `{choiceLog: number[]}` blob, silently re-walking it choice-by-choice on
 * load (`replayWalk`). This provider drives the Rust-canonical session journal
 * instead: persistence is `StorySessionHandle.exportJournal()` (pushed via the
 * `onJournalDirty` hook, never polled or bespoke-timed), and replay-on-load is
 * `StorySessionHandle.restore()`; replay-on-recompile is `session.reload()`.
 * Both return a typed `ReplayOutcome` — divergence is data, not a thrown
 * exception, and truncation happens journal-side (Rust), not in a hand-rolled
 * JS re-walk.
 *
 * **`{choiceLog}` migration (one-time, not a reset):** a legacy blob has no
 * externals/set-vars, only choice indices, so it converts to a journal with
 * `Choice` events by literally replaying it — construct a fresh session on
 * the just-loaded bytes and drive it through the log via `continueToPause`/
 * `choose`, using the exact same truncate-on-divergence contract the old
 * `replayWalk` used (unreachable choice / early end / early DONE / runtime
 * error). This produces a genuine journal (not a synthesized one), which is
 * then exported and persisted in the new format; the old key is removed. A
 * story with recorded externals technically can't reproduce their *results*
 * this way (the migration re-invokes externals live, same as an ordinary
 * fresh replay) — for the studio's ink sandbox (no external bindings) this is
 * moot, and if the choices no longer apply the same divergence + truncation
 * UX fires as it always has.
 *
 * **Post-restore/reload transcript:** the typed `ReplayOutcome` reports
 * divergence/failure structurally (`at_event`, `expected`, `found`) but not
 * accumulated line text — unlike the old `replayWalk`, which re-displayed
 * every replayed line. This provider does not attempt to reconstruct that
 * text: after a restore/reload the transcript starts fresh ("you're just
 * here again", matching a real save/load), and `debugSnapshot()` supplies the
 * live status/choices/position. Pending choices come from
 * `debugSnapshot().pending_choices`, mapped straight across — `DebugChoice`
 * carries its own `index`, which is the *pre-filter* `flow.pending_choices`
 * position (`story.rs`'s `resolved_choices_for`/`build_debug_snapshot` both
 * derive it from the same `.enumerate().filter(!is_invisible_default)` pass
 * as the live `Choice` builder), i.e. exactly the raw index `choose()`
 * expects. It is *not* the post-filter enumeration position — do not
 * re-derive it from array position when a mix of visible and
 * invisible-default choices is possible. `tags` isn't tracked per
 * `DebugChoice` and comes back empty — unused by the player UI today.
 *
 * **`deferred` / `continueToPause()` interaction (#388 checklist):** the
 * studio defines no external bindings and never passes `deferred` names to
 * `StorySessionHandle`'s constructor, so the "`continueToPause`/
 * `continueSingle` silently ignore `deferred`" hazard flagged in the #389
 * review never materializes here — this is the documented consumer-side
 * discipline the checklist asks for, not a runtime guard. A future consumer
 * that *does* want always-deferred externals must drive via `advance()`
 * instead, never `continueToPause()`.
 *
 * **Shared flows (#200)** continue to work: `StorySessionHandle` gained
 * `spawnFlow`/`continueFlow`/`chooseFlow`/`destroyFlow`/`flowDebugSnapshot`
 * (mirroring `StoryRunnerHandle`'s) so a flow spawned here shares *this*
 * session's globals/visits/rng — the same VM instance the session itself
 * drives, not a second one.
 */

import { StorySessionHandle, type ExternalValue } from "@brink-lang/web";
import type {
  Breakpoint,
  Choice,
  DebugRunOutcome,
  DebugLine,
  DebugSourceLocation,
  ProgramAddress,
  ReplayOutcome,
  SessionJournal,
  SessionLine,
  StepMode,
} from "@brink/wasm-types";

import { FlowSessionProvider } from "./flow-provider.js";

import {
  ALL_CAPABILITIES,
  sessionCanContinue,
  statusOfLine,
  transcriptLine,
  transcriptNotice,
  type DebugSessionProvider,
  type TranscriptLine,
  type ProviderCallbacks,
  type SessionCapability,
  type SessionSnapshot,
  type SessionStatus,
} from "./types.js";

const SAVE_KEY = "brink-player-save";

/** Current persisted format: the exported session journal. */
interface JournalSaveData {
  version: 2;
  journal: SessionJournal;
}

/** The pre-#388 persisted format: a bare recorded choice-index log. */
interface LegacySaveData {
  choiceLog: number[];
}

type SaveData = JournalSaveData | LegacySaveData;

function isJournalSave(data: SaveData): data is JournalSaveData {
  return (data as JournalSaveData).version === 2;
}

function saveJournal(journal: SessionJournal): void {
  try {
    const data: JournalSaveData = { version: 2, journal };
    localStorage.setItem(SAVE_KEY, JSON.stringify(data));
  } catch {
    // localStorage may be unavailable
  }
}

function loadFromStorage(): SaveData | null {
  try {
    const raw = localStorage.getItem(SAVE_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as SaveData;
  } catch {
    return null;
  }
}

function clearStorage(): void {
  try {
    localStorage.removeItem(SAVE_KEY);
  } catch {
    // ignore
  }
}

// ── Divergence notification ─────────────────────────────────────────

/**
 * The divergence notification (spec §7.6). Raised as a "warning" from source
 * "story" through the notification service (spec §7.5).
 */
export const REPLAY_DIVERGED_MESSAGE =
  "Story changed — replay diverged; choice history truncated.";

// ── Provider ────────────────────────────────────────────────────────

const NOOP_CALLBACKS: ProviderCallbacks = {
  notify() {
    /* no-op until bound */
  },
  appendOutput() {
    /* no-op until bound */
  },
};

/** Map `debugSnapshot().pending_choices` into `Choice[]` — see the
 * "Post-restore/reload transcript" doc comment above for why `c.index` (the
 * raw pre-filter `pending_choices` position `DebugChoice` carries) is used
 * directly rather than the array position, which would be wrong whenever an
 * invisible-default choice is mixed in at the same pause point. */
function choicesFromDebugState(
  debugState: SessionSnapshot["debugState"],
): Choice[] {
  if (!debugState) return [];
  return debugState.pending_choices.map((c) => ({
    index: c.index,
    text: c.text,
    tags: [],
  }));
}

export class LocalSessionProvider implements DebugSessionProvider {
  readonly kind = "local" as const;
  readonly capabilities: ReadonlySet<SessionCapability> = ALL_CAPABILITIES;

  private session: StorySessionHandle | null;
  private callbacks: ProviderCallbacks;
  private readonly listeners = new Set<(s: SessionSnapshot) => void>();
  /** Unsubscribe from the bound session's `onJournalDirty` hook. */
  private journalUnsub: (() => void) | null = null;

  // Mirrored snapshot fields.
  /**
   * Reveal mode (#3011). `false` — the default — reveals one line at a time.
   * Deliberately NOT persisted with the session journal: it is a view
   * preference about how output is paced, not part of the story's state, and
   * restoring a journal must not silently change how the next reveal behaves.
   */
  private auto = false;
  private status: SessionStatus = "none";
  private transcript: TranscriptLine[] = [];
  /** Paused by the debugger (W5/#3298) — see `SessionSnapshot.paused`. */
  private paused = false;
  /** The most recent debug-advance outcome (W5/#3298). */
  private lastOutcome: DebugRunOutcome | null = null;
  private choices: Choice[] = [];
  private debugState: SessionSnapshot["debugState"] = null;
  private programModel: SessionSnapshot["programModel"] = null;
  private programInkt: string | null = null;
  private programChecksum: string | null = null;

  /** Program bytes this session is running — kept so `restart` can re-create. */
  private bytes: Uint8Array | null = null;

  /**
   * Whether to persist + restore the session journal via localStorage. The
   * primary session persists (restore on reload); secondary local sessions
   * (#182) do not — they're transient, isolated playthroughs that must not
   * clobber the primary's save.
   */
  private persist = true;
  /**
   * Optional entry point for the session: navigate here (`go_to_path`) right
   * after load instead of starting at the root — the "play from here as a new
   * session" path (#182). Secondary sessions only.
   */
  private startPath: { path: string; args: ExternalValue[] } | null = null;

  /**
   * Constructs a fresh session on load — the real `StorySessionHandle`
   * constructor by default. Overridable purely as a test seam (so a test can
   * exercise `start()`'s fresh-load path, including legacy-log migration,
   * against a scriptable fake instead of a real wasm session).
   */
  private readonly sessionFactory: (bytes: Uint8Array) => StorySessionHandle;

  constructor(opts?: {
    callbacks?: ProviderCallbacks;
    /** Adopt an already-live session (the studio wraps an existing handle; tests). */
    session?: StorySessionHandle;
    /** Status of an adopted session (default "running"). */
    status?: SessionStatus;
    /** Transcript of an adopted session (default empty). */
    transcript?: string[];
    /** Pending choices of an adopted session (default empty). */
    choices?: Choice[];
    /** Persist + restore the session journal (default true; false for secondary sessions). */
    persist?: boolean;
    /** Navigate to this entry point after load instead of the root (#182). */
    startPath?: { path: string; args?: ExternalValue[] };
    /** Test seam: override how a fresh session is constructed in `start()`. */
    sessionFactory?: (bytes: Uint8Array) => StorySessionHandle;
  }) {
    this.callbacks = opts?.callbacks ?? NOOP_CALLBACKS;
    this.session = opts?.session ?? null;
    this.persist = opts?.persist ?? true;
    this.sessionFactory =
      opts?.sessionFactory ?? ((bytes) => new StorySessionHandle(bytes));
    this.startPath = opts?.startPath
      ? { path: opts.startPath.path, args: opts.startPath.args ?? [] }
      : null;
    if (opts?.session) {
      this.status = opts.status ?? "running";
      this.transcript = (opts.transcript ?? []).map((t) =>
        typeof t === "string" ? transcriptLine(t) : t,
      );
      this.choices = opts.choices ?? [];
      this.watchJournal(opts.session);
    }
  }

  /** Wire the studio services. Called by the session slice at bind time. */
  setCallbacks(callbacks: ProviderCallbacks): void {
    this.callbacks = callbacks;
  }

  /** Whether a live session exists (drives restart-vs-fresh-start; see slice). */
  hasLiveRunner(): boolean {
    return this.session !== null;
  }

  /**
   * Spawn a shared-context flow on this session's VM (#200): a concurrent
   * flow of the *same* story that shares globals / visits / rng. Returns a
   * {@link FlowSessionProvider} that drives it, or `null` if there's no live
   * session. The flow shares this provider's wired callbacks.
   */
  spawnFlow(name: string, path?: string): FlowSessionProvider | null {
    if (!this.session) return null;
    this.session.spawnFlow(name, path);
    return new FlowSessionProvider(this.session, name, this.callbacks);
  }

  // ── SessionProvider ───────────────────────────────────────────────

  getSnapshot(): SessionSnapshot {
    return {
      status: this.status,
      transcript: this.transcript,
      choices: this.choices,
      debugState: this.debugState,
      programChecksum: this.programChecksum,
      programModel: this.programModel,
      programInkt: this.programInkt,
      paused: this.paused,
      debugOutcome: this.lastOutcome,
      auto: this.auto,
    };
  }

  subscribe(listener: (snapshot: SessionSnapshot) => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  start(bytes?: Uint8Array): void {
    if (!bytes) return; // local provider requires program bytes (spec §3)

    try {
      const prev = this.session;
      let session: StorySessionHandle;
      let outcome: ReplayOutcome | null = null;

      if (prev) {
        // In-place hot-reload: replays this session's own journal against the
        // recompiled program (spec's replay-on-recompile path).
        try {
          outcome = prev.reload(bytes);
          session = prev;
        } catch {
          // `reload()` throws on decode/link failure of the recompiled bytes
          // (rather than returning a `ReplayOutcome`) — `prev` is left
          // untouched by the wasm side in that case, but it's still the old
          // program's session and can't be reused. Free it and fall back to
          // a fresh session on the new bytes (dropping the journal), matching
          // the pre-migration provider's recovery path — don't let this leak
          // the old wasm handle or dead-end the player in an error state.
          prev.free();
          // Clear the field immediately: `this.session` still points at the
          // now-freed `prev` until `bindSession` reassigns it below, and if
          // `sessionFactory` itself throws next, the outer catch must not
          // free this same handle a second time.
          this.session = null;
          session = this.sessionFactory(bytes);
        }
      } else {
        session = this.sessionFactory(bytes);
      }
      this.bindSession(session);
      this.bytes = bytes;

      // The program inspection is static for the program — capture once on load.
      this.programModel = this.capture(() => session.programModel());
      this.programInkt = this.capture(() => session.programInkt());
      this.programChecksum = this.programModel?.checksum ?? null;

      // A secondary "play from here" session jumps to its entry point before
      // revealing — no persisted restore (it doesn't persist). It is a dev
      // affordance, so it overrides #@private visibility enforcement (M-2b):
      // the entry point may be a private knot.
      if (this.startPath) {
        session.setDevVisibilityOverride(true);
        session.goToPath(this.startPath.path, ...this.startPath.args);
        this.reveal();
        return;
      }

      if (outcome) {
        // A hot-reload just ran on the live session's own journal.
        this.applyReplayOutcome(outcome);
        return;
      }

      // Fresh session: check for persisted data.
      const saved = this.persist ? loadFromStorage() : null;
      if (saved && isJournalSave(saved)) {
        this.restoreFromJournal(bytes, saved.journal);
      } else if (saved && "choiceLog" in saved && saved.choiceLog.length > 0) {
        this.migrateLegacyChoiceLog(saved.choiceLog);
      } else {
        this.reveal();
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      // Don't leak a live wasm handle: whatever session is currently bound
      // (including one just bound earlier in this same `start()` call) must
      // be freed before we drop the reference.
      if (this.session) {
        this.unwatchJournal();
        this.session.free();
      }
      this.session = null;
      this.bytes = null;
      this.status = "error";
      this.transcript = [transcriptNotice(`Load error: ${msg}`)];
      this.choices = [];
      this.programModel = null;
      this.programInkt = null;
      this.programChecksum = null;
      this.debugState = null;
      this.callbacks.appendOutput("story", `Load error: ${msg}`);
      this.emit();
    }
  }

  restart(): void {
    if (!this.session) {
      // No live session (e.g. a prior load error or a stop) — restart means a
      // fresh start on the bytes this session last ran.
      if (this.bytes) this.start(this.bytes);
      return;
    }
    this.session.restart();
    if (this.persist) clearStorage();
    this.status = "running";
    this.transcript = [];
    this.choices = [];
    // A restart abandons the debug pause point — it belongs to the old run.
    this.paused = false;
    this.lastOutcome = null;
    // Re-navigate a "play from here" session to its entry on restart — still a
    // dev affordance, so keep the #@private visibility override on (M-2b).
    if (this.startPath) {
      this.session.setDevVisibilityOverride(true);
      this.session.goToPath(this.startPath.path, ...this.startPath.args);
    }
    this.reveal();
  }

  stop(): void {
    this.unwatchJournal();
    if (this.session) this.session.free();
    this.session = null;
    // Stopping ends the session *intent* — a later `start` is a fresh run, so
    // the persisted journal goes too.
    if (this.persist) clearStorage();
    this.status = "none";
    this.transcript = [];
    this.choices = [];
    this.debugState = null;
    // programModel / programInkt / programChecksum / bytes are kept — the
    // program identity outlives the run (start restarts it; the Program
    // Explorer is compile-bound, not session-bound).
    this.emit();
  }

  choose(index: number): void {
    const session = this.session;
    if (!session) return;

    const choiceText = this.choices.find((c) => c.index === index)?.text;

    try {
      session.choose(index);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      this.status = "error";
      this.transcript = [...this.transcript, transcriptNotice(`Choose error: ${msg}`)];
      this.choices = [];
      this.callbacks.appendOutput("story", `Choose error: ${msg}`);
      this.emit();
      return;
    }

    // Append the chosen text as a marker, clear choices.
    if (choiceText)
      this.transcript = [...this.transcript, { text: `> ${choiceText}`, kind: "marker", tags: [] } satisfies TranscriptLine];
    this.choices = [];

    // Reveal the next section (emits). The journal-dirty hook handles
    // persistence — no bespoke save call here. Choosing while paused
    // STAYS paused (F7's ruled choice presentation) — only the Continue/
    // reveal gesture resumes (2026-08-30 ruling), and this is not it.
    this.reveal(this.paused);
  }

  /**
   * Switch between one-line and run-to-pause reveals (#3011).
   *
   * Takes effect on the NEXT reveal. It does not re-run or collapse what is
   * already in the transcript — turning `auto` on mid-scene continues from
   * where the reader is, rather than replaying the scene at a different
   * granularity.
   *
   * Emits even though no story state moved, because the snapshot carries
   * `auto` and the checkbox reads its state from there.
   */
  setAuto(auto: boolean): void {
    if (this.auto === auto) return;
    this.auto = auto;
    this.emit();
  }

  continue(): void {
    // Only advance when the session actually can (mid-flow or at a `-> DONE`
    // turn boundary). At a choice point the VM is blocked awaiting input, so a
    // stray `story.continue` (a late click as choices appear) must be a no-op —
    // otherwise it re-reveals and the status briefly flips off `awaiting-choice`,
    // making the player flicker between the choice list and a Continue button
    // (#273). The command's `when` already gates the button; this makes the
    // provider authoritative too.
    if (!sessionCanContinue(this.status)) return;
    this.reveal();
  }

  dispose(): void {
    this.unwatchJournal();
    if (this.session) this.session.free();
    this.session = null;
    this.bytes = null;
    this.listeners.clear();
    this.status = "none";
    this.transcript = [];
    this.choices = [];
    this.debugState = null;
    this.programModel = null;
    this.programInkt = null;
    this.programChecksum = null;
    this.paused = false;
    this.lastOutcome = null;
  }

  // ── DebugSessionProvider (D8, #3186 — control-half bridge, #3232) ──
  //
  // Delegates straight onto `StorySessionHandle`'s own bindings — this
  // provider is exactly the "the actual live-session consumer" `debugRun`'s
  // wasm doc names. `debugRun`/`debugStep` additionally refresh + emit the
  // mirrored snapshot afterward (same as `reveal()`), since a breakpoint/
  // step lands the runtime at a new position the State View must reflect.

  // The resolver family runs on RENDER paths (the paused chip's selector,
  // the editor's highlight callback), so it must never throw: a freed
  // wasm handle raises "null pointer passed to rust" from inside the
  // binding — a nullish check can't see it (found live: a disposed
  // duplicate dev mount crash-looped React through exactly this).
  // `capture()` is the same mid-teardown guard `refreshDebug` uses.

  resolveDebugPosition(containerIdx: number, offset: number): DebugSourceLocation | null {
    return this.capture(() => this.session?.resolveDebugPosition(containerIdx, offset) ?? null);
  }

  resolveSourceLine(file: string, line: number): ProgramAddress | null {
    return this.capture(() => this.session?.resolveSourceLine(file, line) ?? null);
  }

  hasDebugInfo(): boolean {
    return this.capture(() => this.session?.hasDebugInfo() ?? false) ?? false;
  }

  resolveDebugLine(containerIdx: number, offset: number): DebugLine | null {
    return this.capture(() => this.session?.resolveDebugLine(containerIdx, offset) ?? null);
  }

  debugBreakpointAdd(containerIdx: number, offset: number, name?: string): number {
    // -1 is not a real id `BreakpointSet::insert` ever returns (ids start at
    // 0) — an unmistakable "no live session to arm this on" sentinel rather
    // than silently pretending to add one.
    return this.session?.debugBreakpointAdd(containerIdx, offset, name) ?? -1;
  }

  debugBreakpointRemove(id: number): boolean {
    return this.session?.debugBreakpointRemove(id) ?? false;
  }

  debugBreakpointSetEnabled(id: number, enabled: boolean): boolean {
    return this.session?.debugBreakpointSetEnabled(id, enabled) ?? false;
  }

  debugBreakpoints(): Breakpoint[] {
    return this.session?.debugBreakpoints() ?? [];
  }

  debugRun(budgetCeiling?: number): DebugRunOutcome {
    const session = this.session;
    // No live session: nothing to run — reported the same way `debug_run`
    // itself reports "nothing left to do", so a caller need not special-case
    // "no session" vs. "session already at its end."
    if (!session) return { reason: { type: "terminal" }, depth: 0, lines: [] };
    // Continue (F5): free-run resumes — paused clears unless the run stops
    // at another breakpoint (applyDebugOutcome re-sets it then).
    this.paused = false;
    const outcome = session.debugRun(budgetCeiling);
    this.applyDebugOutcome(outcome, false);
    this.emit();
    return outcome;
  }

  debugStep(mode: StepMode, budgetCeiling?: number): DebugRunOutcome {
    const session = this.session;
    if (!session) return { reason: { type: "terminal" }, depth: 0, lines: [] };
    const outcome = session.debugStep(mode, budgetCeiling);
    // An explicit step leaves the session paused — stepping IS the paused
    // mode's way of moving (W5/#3298).
    this.applyDebugOutcome(outcome, true);
    this.emit();
    return outcome;
  }

  debugStepLine(mode: StepMode, budgetCeiling?: number): DebugRunOutcome {
    const session = this.session;
    if (!session) return { reason: { type: "terminal" }, depth: 0, lines: [] };
    const outcome = session.debugStepLine(mode, budgetCeiling);
    this.applyDebugOutcome(outcome, true);
    this.emit();
    return outcome;
  }

  debugRunToLine(budgetCeiling?: number): DebugRunOutcome {
    const session = this.session;
    if (!session) return { reason: { type: "terminal" }, depth: 0, lines: [] };
    // Continue (2026-08-30 ruling): deliver the next content line and
    // RESUME play — paused clears unless the run stops at a breakpoint
    // (applyDebugOutcome re-sets it then).
    this.paused = false;
    const outcome = session.debugRunToLine(budgetCeiling);
    this.applyDebugOutcome(outcome, false);
    this.emit();
    return outcome;
  }

  pause(): void {
    // The pause verb (W5/#3298): enter the paused state at the current
    // boundary. Reveals are user-driven in this architecture, so there is
    // never a mid-flight run to interrupt — pausing here makes the NEXT
    // advance a bounded line step and lights the step controls.
    if (this.paused || !this.session) return;
    this.paused = true;
    this.refreshDebug();
    this.emit();
  }

  // ── Internals ─────────────────────────────────────────────────────

  private capture<T>(fn: () => T): T | null {
    try {
      return fn();
    } catch {
      return null;
    }
  }

  /** Wire the persistence push signal (`onJournalDirty`) on a freshly bound
   * session, tearing down any previous subscription first. */
  private bindSession(session: StorySessionHandle): void {
    this.unwatchJournal();
    this.session = session;
    this.watchJournal(session);
  }

  private watchJournal(session: StorySessionHandle): void {
    if (!this.persist) return;
    this.journalUnsub = session.onJournalDirty(() => {
      try {
        saveJournal(session.exportJournal());
      } catch {
        // localStorage may be unavailable; persistence is best-effort.
      }
    });
  }

  private unwatchJournal(): void {
    this.journalUnsub?.();
    this.journalUnsub = null;
  }

  /**
   * Reveal from the runtime and emit.
   *
   * ONE line by default; the whole run to the next pause when `auto` is on
   * (#3011, ruled 2026-08-23 in `docs/decision-log.md`).
   *
   * This method previously called `continueToPause()` unconditionally while
   * its own doc comment claimed it revealed "the next line" — the comment
   * described the intended behaviour and the code did something else, so a
   * Continue press dumped every line up to the next choice. That is wrong for
   * an authoring tool: it makes it impossible to see where a line lands or
   * which convention fired on it.
   *
   * Both branches yield `SessionLine[]`, so everything downstream — the
   * transcript append, `statusOfLine` on the last element, the choices pull —
   * is untouched. `continueSingle()` returns a single line that may ITSELF be
   * terminal (`choices`/`done`/`end`), which is why wrapping it in an array
   * needs no special-casing.
   */
  private reveal(stayPaused = false): void {
    const session = this.session;
    if (!session) {
      this.emit();
      return;
    }

    try {
      if (this.debugDriven()) {
        // W5/#3298 — play and debug are ONE loop: with breakpoints armed
        // (or the session paused), the production continue path can never
        // hit them, so advancement routes through the debug verbs. A
        // single reveal runs to the next CONTENT line bounded by
        // breakpoints (2026-08-30 Continue ruling — the reveal-while-
        // paused click IS Continue, and it RESUMES play; the choose road
        // passes `stayPaused` to keep F7's paused choice presentation);
        // auto is a free run to the next breakpoint/choice/terminal.
        // Debug advances bypass the journal by ruled design — choices
        // stay journaled, so replay/restore still reconstructs to the
        // same turn boundary, only a paused-mid-turn position is not
        // itself restorable.
        this.advanceDebug(this.auto && !this.paused ? "run" : "line", stayPaused);
      } else {
        const lines = this.auto ? session.continueToPause() : [session.continueSingle()];
        this.appendLines(lines);
        const last = lines.at(-1);
        this.status = last ? statusOfLine(last.type) : this.status;
        this.choices = last?.type === "choices" ? (last.choices ?? []) : [];
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      this.status = "error";
      this.transcript = [...this.transcript, transcriptNotice(`Runtime error: ${msg}`)];
      this.choices = [];
      this.callbacks.appendOutput("story", `Runtime error: ${msg}`);
    }

    this.refreshDebug();
    this.emit();
  }

  private appendLines(lines: SessionLine[]): void {
    for (const line of lines) {
      const text = line.text.replace(/\n$/, "");
      if (text)
        this.transcript = [
          ...this.transcript,
          transcriptLine(text, line.tags, line.source),
        ];
    }
  }

  /** Whether advancement must route through the debug verbs (W5/#3298):
   *  armed breakpoints only ever hit inside the debug loop, and a paused
   *  session resumes through it. */
  private debugDriven(): boolean {
    if (this.paused) return true;
    try {
      return (this.session?.debugBreakpoints().length ?? 0) > 0;
    } catch {
      return false;
    }
  }

  /** One debug-driven advance: `"line"` = the author-tier content-line
   *  run (2026-08-30 Continue ruling — deliver the next content line,
   *  bounded by breakpoints, needs no debug line info), `"run"` =
   *  free-run to the next breakpoint/choice/terminal. `stayPaused` keeps
   *  the paused state across an ordinary stop; a breakpoint/watchpoint
   *  hit pauses regardless. */
  private advanceDebug(kind: "run" | "line", stayPaused: boolean): void {
    const session = this.session;
    if (!session) return;
    const outcome =
      kind === "run" ? session.debugRun() : session.debugRunToLine();
    this.applyDebugOutcome(outcome, stayPaused);
  }

  /** Fold a debug outcome into the mirrored session state: transcript
   *  delta, paused-ness, status, choices. */
  private applyDebugOutcome(outcome: DebugRunOutcome, stayPaused: boolean): void {
    this.lastOutcome = outcome;
    for (const line of outcome.lines) {
      const text = line.text.replace(/\n$/, "");
      if (text)
        this.transcript = [
          ...this.transcript,
          transcriptLine(text, line.tags, line.source),
        ];
    }
    this.refreshDebug();
    switch (outcome.reason.type) {
      case "breakpoint":
      case "watchpoint":
        this.paused = true;
        this.status = "running";
        this.choices = [];
        break;
      case "choices":
        // Choices and debug share one presentation (spec F7): picking a
        // choice while paused stays paused.
        this.paused = stayPaused;
        this.status = "awaiting-choice";
        this.choices = choicesFromDebugState(this.debugState);
        break;
      case "terminal":
        this.paused = false;
        this.status = this.debugState
          ? statusOfSnapshotStatus(this.debugState.status)
          : "done";
        this.choices = [];
        break;
      default:
        // step / noStepOutTarget / awaitingExternal: position moved (or
        // honestly refused); paused-ness follows the caller's intent.
        this.paused = stayPaused;
        this.status = "running";
        this.choices = [];
        break;
    }
  }

  private refreshDebug(): void {
    const session = this.session;
    if (!session) {
      this.debugState = null;
      return;
    }
    try {
      this.debugState = session.debugSnapshot();
    } catch {
      // The session can be mid-teardown or in an error state — never let the
      // debug snapshot throw into the UI.
      this.debugState = null;
    }
  }

  private notifyDiverged(): void {
    this.callbacks.notify({
      severity: "warning",
      source: "story",
      message: REPLAY_DIVERGED_MESSAGE,
    });
  }

  private emit(): void {
    const snap = this.getSnapshot();
    for (const listener of this.listeners) listener(snap);
  }

  /**
   * Apply a `ReplayOutcome` from `reload()`/`restore()`: mirror wherever the
   * session landed into the snapshot fields. No accumulated transcript text
   * is reconstructed (see the class doc comment) — a fresh session doesn't
   * add anything ("you're just here"); a hot-reload keeps whatever transcript
   * was already showing and only updates status/choices/position.
   */
  private applyReplayOutcome(outcome: ReplayOutcome): void {
    this.refreshDebug();
    this.choices = choicesFromDebugState(this.debugState);
    this.status = this.debugState
      ? statusOfSnapshotStatus(this.debugState.status)
      : "error";

    switch (outcome.type) {
      case "replayed":
        // Clean replay (possibly with soft label-drift warnings) — no
        // divergence notification.
        break;
      case "diverged":
        this.notifyDiverged();
        break;
      case "failed":
        if (outcome.reason.type === "runtime_error") {
          this.status = "error";
          this.transcript = [
            ...this.transcript,
            transcriptNotice(`Runtime error: ${outcome.reason.message}`),
          ];
          this.callbacks.appendOutput(
            "story",
            `Runtime error: ${outcome.reason.message}`,
          );
        }
        this.notifyDiverged();
        break;
    }
    this.emit();
  }

  /** Restore a persisted session journal (the new format). */
  private restoreFromJournal(bytes: Uint8Array, journal: SessionJournal): void {
    const { session, outcome } = StorySessionHandle.restore(bytes, journal);
    this.bindSession(session);
    this.applyReplayOutcome(outcome);
  }

  /**
   * One-time migration of the pre-#388 `{choiceLog}` blob: replay it against
   * the freshly constructed session exactly like the old provider's
   * `replayWalk` did, but building a *real* journal along the way (choices go
   * through `session.choose()`, so they're journaled as they're applied).
   * Once the walk settles (clean finish, or truncated at a divergence point,
   * matching the old UX byte-for-byte), the resulting journal is exported and
   * persisted in the new format, and the legacy key is dropped.
   */
  private migrateLegacyChoiceLog(choiceLog: number[]): void {
    const session = this.session;
    if (!session) return;

    const allText: TranscriptLine[] = [];
    let choiceIdx = 0;

    const finishAndPersist = (): void => {
      this.transcript = allText;
      this.refreshDebug();
      this.emit();
      if (this.persist) saveJournal(session.exportJournal());
    };

    const diverge = (): void => {
      this.notifyDiverged();
      finishAndPersist();
    };

    // Cap iterations at the number of saved choices (+1 margin) so a story
    // that dead-ends on DONE before reaching the next saved choice can't spin
    // forever (unbounded-growth guard, mirroring the old provider's).
    let budget = choiceLog.length + 1;
    while (choiceIdx < choiceLog.length) {
      if (budget-- <= 0) {
        clearStorage();
        this.notifyDiverged();
        this.reveal();
        return;
      }

      let lines: SessionLine[];
      try {
        lines = session.continueToPause();
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        this.status = "error";
        this.choices = [];
        this.callbacks.appendOutput("story", `Runtime error: ${msg}`);
        allText.push(transcriptNotice(`Runtime error: ${msg}`));
        diverge();
        return;
      }

      for (const line of lines) {
        const text = line.text.replace(/\n$/, "");
        if (text) allText.push(transcriptLine(text, line.tags, line.source));
      }

      const last = lines.at(-1);
      if (last?.type === "choices") {
        const savedChoice = choiceLog[choiceIdx];
        const offered = last.choices ?? [];
        let chose = false;
        if (savedChoice !== undefined && offered.some((c) => c.index === savedChoice)) {
          try {
            session.choose(savedChoice);
            chose = true;
          } catch {
            chose = false;
          }
        }

        if (!chose) {
          // The recorded index is no longer valid — divergence. Stay at this
          // choice point and let the user pick from what is offered now.
          this.transcript = allText;
          this.choices = offered;
          this.status = "awaiting-choice";
          diverge();
          return;
        }

        const choiceText = offered.find((c) => c.index === savedChoice)?.text;
        if (choiceText) allText.push({ text: `> ${choiceText}`, kind: "marker", tags: [] });
        choiceIdx += 1;
        continue;
      }

      if (last?.type === "end") {
        this.transcript = allText;
        this.choices = [];
        this.status = "ended";
        diverge();
        return;
      }

      // The pass produced no choice to consume and didn't end (a `-> DONE`
      // dead-end) — the next recorded choice is unreachable. Divergence.
      this.transcript = allText;
      this.choices = [];
      this.status = last ? statusOfLine(last.type) : "done";
      diverge();
      return;
    }

    // All choices replayed — show accumulated text and reveal the next line,
    // then persist the fresh journal (no divergence, nothing to notify).
    this.transcript = allText;
    this.reveal();
    if (this.persist) saveJournal(session.exportJournal());
  }
}

/** Map a `StateSnapshot`/`DebugState`-style status string to `SessionStatus`. */
function statusOfSnapshotStatus(status: string): SessionStatus {
  switch (status) {
    case "waiting_for_choice":
      return "awaiting-choice";
    case "ended":
      return "ended";
    case "done":
      return "done";
    default:
      return "running";
  }
}
