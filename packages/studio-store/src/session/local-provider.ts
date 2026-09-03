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

import {
  StoryRunnerHandle,
  StorySessionHandle,
  type ExternalValue,
  type SpeculationResult,
} from "@brink-lang/web";
import type {
  Breakpoint,
  Choice,
  DebugRunOutcome,
  DebugLine,
  DebugSourceLocation,
  LoadReport,
  ProgramAddress,
  ReplayOutcome,
  SaveState,
  SessionJournal,
  ProjectSource,
  SessionLine,
  SourceLocation,
  StepMode,
  StructuralTranscript,
} from "@brink/wasm-types";

import { FlowSessionProvider } from "./flow-provider.js";

import {
  ALL_CAPABILITIES,
  sessionCanContinue,
  statusOfLine,
  transcriptLine,
  transcriptNotice,
  type DebugSessionProvider,
  type PeekResult,
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
    ...(c.sticky === undefined ? {} : { sticky: c.sticky }),
    ...(c.source === undefined ? {} : { source: c.source }),
  }));
}

/** The runtime's line limit per turn (`FlowInstance::LINE_LIMIT`), mirrored
 *  as the cap on the single-line stepping loops — guard against unbounded
 *  growth on this side too. */
const STEP_LINE_LIMIT = 10_000;

/** `currentPath()` where the session has it; `null` on older stubs/hosts. */
function currentPathOf(session: { currentPath?: () => string | null }): string | null {
  return typeof session.currentPath === "function" ? session.currentPath() : null;
}

/** The transcript's echo of a taken choice (#3435): the text as before,
 *  plus how it was written and where it came from, when the wire says. */
function choiceEcho(text: string, chosen: Choice | undefined): TranscriptLine {
  const line: TranscriptLine = { text: `> ${text}`, kind: "marker", tags: [] };
  if (chosen?.sticky !== undefined) line.choiceKind = chosen.sticky ? "sticky" : "once";
  if (chosen?.source !== undefined) line.source = chosen.source;
  return line;
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
  /** Paced auto-reveal (W7/#3300 F13, RULED): with auto on and a
   * positive delay, a Continue reveals the run ONE line at a time in
   * rapid succession instead of one batch. 0 = all at once. */
  private pacedDelayMs = 0;
  private pacedTimer: ReturnType<typeof setTimeout> | null = null;
  /** Watch (W17/#3310): the scratch runner fragment/expression evals run
   * over — an independent wasm object seeded from the session's durable
   * state per round, re-keyed per program version so the runner's
   * fragment-compile cache pays each entry once per compile. Nothing it
   * does can touch the live session (discard-on-drop speculation). */
  private watchRunner: StoryRunnerHandle | null = null;
  private watchRunnerChecksum: string | null = null;
  /** Unix ms of the last successful hot-reload migration/replay (W15) —
   * the Player chip's brief "reloaded" affirmation reads it. */
  private reloadedAt: number | null = null;
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
      reloadedAt: this.reloadedAt,
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
      // W15/#3308 (RULED): snapshot durable state BEFORE touching the old
      // session — the migration fallback's material. Replay stays the
      // primary road (it preserves the exact position and transcript);
      // the snapshot catches what replay can't.
      let migrateState: SaveState | null = null;
      let migrateKnot: string | null = null;
      let migrateTurn = 0;
      let migrateTranscript: StructuralTranscript | null = null;

      if (prev) {
        migrateState = this.capture(() => prev.saveState());
        // The story-so-far, structurally (RULED 2026-08-30) — captured
        // BEFORE the reload touches `prev`, re-rendered on the session
        // that survives it, against the NEW program's line tables.
        migrateTranscript = this.capture(() => prev.exportTranscript());
        migrateKnot = this.debugState?.current_location ?? null;
        migrateTurn = this.debugState?.turn_index ?? 0;
        this.stopPacedPump();
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
        if (outcome.type === "replayed") {
          // A "clean" replay can still be a LIE by omission: debug-driven
          // advances bypass the session journal (W5's ruled design — and
          // today even choices made at a debug-road stop journal nothing,
          // #3334), so a session that played under armed breakpoints
          // replays to an EARLIER turn than it was actually at. Detect
          // the regression and migrate the durable state instead.
          const replayedTurn = this.capture(() => session.debugSnapshot())?.turn_index ?? 0;
          if (
            replayedTurn < migrateTurn &&
            migrateState !== null &&
            this.migrateInto(migrateState, migrateKnot, migrateTranscript)
          ) {
            return;
          }
          // Genuinely clean: exact position + transcript survive — the
          // best case. Flash the chip's "reloaded" affirmation (W15).
          this.reloadedAt = Date.now();
          this.applyReplayOutcome(outcome);
          return;
        }
        // Replay diverged or failed (W15): migrate the DURABLE state
        // instead of truncating — globals/visits/turn survive the edit;
        // the position drops to the recorded knot (honest fallback).
        if (
          migrateState !== null &&
          this.migrateInto(migrateState, migrateKnot, migrateTranscript)
        ) {
          return;
        }
        // No snapshot to migrate (or migration itself failed): the old
        // truncation road, notification and all.
        this.applyReplayOutcome(outcome);
        return;
      }

      // The reload threw and a FRESH session replaced the old one: the
      // journal is gone, but the durable state can still migrate (W15) —
      // previously this dropped everything.
      if (
        prev &&
        migrateState !== null &&
        this.migrateInto(migrateState, migrateKnot, migrateTranscript)
      ) {
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
    this.stopPacedPump();
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
    this.stopPacedPump();
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

    const chosen = this.choices.find((c) => c.index === index);
    const choiceText = chosen?.text;

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
    if (choiceText) this.transcript = [...this.transcript, choiceEcho(choiceText, chosen)];
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
    // Turning auto OFF mid-run abandons the paced pump immediately.
    if (!auto) this.stopPacedPump();
    this.emit();
  }

  /** Configure the paced auto-reveal cadence (0 disables — batch mode). */
  setPacedReveal(delayMs: number): void {
    this.pacedDelayMs = Math.max(0, delayMs);
    if (this.pacedDelayMs === 0) this.stopPacedPump();
  }

  /** Whether a paced run is currently pumping (test/UI observability). */
  pacedRunning(): boolean {
    return this.pacedTimer !== null;
  }

  private stopPacedPump(): void {
    if (this.pacedTimer !== null) {
      clearTimeout(this.pacedTimer);
      this.pacedTimer = null;
    }
  }

  /** One paced tick: reveal a single content line, then keep pumping
   * while the session is still plainly running — a choice point, a
   * terminal, an error, a breakpoint hit, or an explicit pause all end
   * the run (the paused check is the ruled instant flush: nothing is
   * queued, so stopping the pump IS the flush). */
  private pacedTick(): void {
    this.pacedTimer = null;
    if (!this.session || this.paused || this.status !== "running") return;
    this.revealOne();
    if (this.status === "running" && !this.paused && this.pacedDelayMs > 0) {
      this.pacedTimer = setTimeout(() => {
        this.pacedTick();
      }, this.pacedDelayMs);
    }
  }

  /**
   * One-shot fast-forward (RULED 2026-08-30): run to the next stop —
   * choices, breakpoint, terminal — ink's `ContinueMaximally` shape.
   * Delivery honors the paced/all-at-once App setting; nothing sticky:
   * the next ordinary reveal is single-line again (equivalent to
   * enable-auto → continue → re-disable-auto, as one gesture).
   */
  continueMaximally(): void {
    if (!sessionCanContinue(this.status)) return;
    const session = this.session;
    if (!session) return;
    if (this.pacedDelayMs > 0) {
      // Paced: reveal now, keep pumping until a stop — the pump's own
      // conditions (status/paused) end it; no auto flag involved.
      this.revealOne();
      if (this.status === "running" && !this.paused) {
        this.stopPacedPump();
        this.pacedTimer = setTimeout(() => {
          this.pacedTick();
        }, this.pacedDelayMs);
      }
      return;
    }
    // All at once: the batch road (debug-driven when breakpoints are
    // armed, so a hit still stops the run).
    try {
      if (this.debugDriven()) {
        this.advanceDebug("run", false);
      } else {
        const last = this.stepToPause(session);
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

  /**
   * The journaled batch road, one line at a time (ruled 2026-09-02, "TS
   * steps single lines"): each line is stamped with the knot/stitch the
   * runtime reported BEFORE the continue that delivered it — where the
   * line comes from (`currentPath()` is where the story IS, like ink's
   * `currentPathString`). Stops at the first non-text line, and at the
   * runtime's own line limit, so a runaway story cannot spin here.
   * Returns the last line delivered.
   */
  private stepToPause(session: StorySessionHandle): SessionLine | undefined {
    let last: SessionLine | undefined;
    for (let i = 0; i < STEP_LINE_LIMIT; i++) {
      const path = currentPathOf(session);
      last = session.continueSingle();
      this.appendLines([last], path);
      if (last.type !== "text") break;
    }
    return last;
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
    this.watchRunner?.free();
    this.watchRunner = null;
    this.stopPacedPump();
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

  /** Break-on-write (W18/#3311): arm/disarm/list — see the handle's
   * docs. The provider is a thin pass-through; the paused/refresh dance
   * belongs to the advance that HITS one, not the arming. */
  debugWatchpointAdd(name: string): boolean {
    return this.capture(() => this.session?.debugWatchpointAdd(name) ?? false) ?? false;
  }

  debugWatchpointRemove(name: string): boolean {
    return this.capture(() => this.session?.debugWatchpointRemove(name) ?? false) ?? false;
  }

  debugWatchpoints(): string[] {
    return this.capture(() => this.session?.debugWatchpoints() ?? []) ?? [];
  }

  /** Watch evaluation (W17/#3310, spec §F18): run one entry against the
   * session's CURRENT durable state. The scratch runner is seeded via
   * `saveState()` (name-keyed — survives the cross-program hop), then
   * `evaluate()` runs the shipped tiering: knot paths and literal-arg
   * calls speculate directly; anything else takes the Tier-1
   * fragment-compile road (cached per program version), for which the
   * caller supplies `projectSource`. `null` = no live session. */
  evaluateWatch(
    source: string,
    opts?: { projectSource?: ProjectSource; budget?: { steps?: number; lines?: number } },
  ): Promise<SpeculationResult> | null {
    const session = this.session;
    if (!session || this.bytes === null) return null;
    const state = this.capture(() => session.saveState());
    if (state === null) return null;
    try {
      if (this.watchRunner === null || this.watchRunnerChecksum !== this.programChecksum) {
        this.watchRunner?.free();
        this.watchRunner = new StoryRunnerHandle(this.bytes);
        this.watchRunnerChecksum = this.programChecksum;
      }
      this.watchRunner.load(state);
      // A `-> knot.stitch` entry means "preview what this divert WOULD
      // produce" (spec §F18's own example) — strip the sigil so it takes
      // Tier-0's goToPath road (transcript preview). Left intact, the
      // Tier-1 expression attempt succeeds FIRST as a divert-target
      // LITERAL (`-> x` is a valid expression) and the row would show
      // the target's name instead of the preview (measured live).
      const divert = /^->\s*([A-Za-z_][\w.]*)\s*$/.exec(source.trim());
      return this.watchRunner.evaluate(divert ? divert[1] : source, {
        projectSource: opts?.projectSource,
        budget: opts?.budget,
      });
    } catch {
      return null;
    }
  }

  /** Live value editing (W16/#3309, RULED: paused-only, scalars only) —
   * a GLOBAL. Enforces the paused gate here (the runtime seam itself
   * doesn't care); `false` = refused, nothing written (the panel's
   * red-shake). A successful edit refreshes the debug mirror so the
   * changed-row highlight lights. */
  editGlobal(name: string, input: string): boolean {
    if (!this.session || !this.paused) return false;
    const ok = this.capture(() => this.session?.debugEditGlobal(name, input)) ?? false;
    if (ok) {
      this.refreshDebug();
      this.emit();
    }
    return ok;
  }

  /** Live value editing — a frame LOCAL, addressed by the snapshot's
   * innermost-first frame index + slot. Same paused gate and refusal
   * contract as `editGlobal`. The panel additionally disables locals at
   * `waiting_for_choice` (choosing restores the choice's captured thread,
   * which would silently overwrite the edit — measured in brink-web's
   * value_edit tests). */
  editTemp(frameIdx: number, slot: number, input: string): boolean {
    if (!this.session || !this.paused) return false;
    const ok =
      this.capture(() => this.session?.debugEditTemp(frameIdx, slot, input)) ?? false;
    if (ok) {
      this.refreshDebug();
      this.emit();
    }
    return ok;
  }

  /** Capture the durable game state (W14/#3307) — `null` without a live
   * session (the wasm handle throws through `capture`'s guard). */
  saveState(): SaveState | null {
    return this.capture(() => this.session?.saveState() ?? null);
  }

  /** Export the structural transcript (RULED 2026-08-30) — the part
   * stream a save carries, re-renderable against any later compile. */
  exportTranscript(): StructuralTranscript | null {
    return this.capture(() => this.session?.exportTranscript() ?? null);
  }

  /** Hot-reload migration (W15/#3308): the checkpoint road with the
   * "reloaded" framing — restore the snapshot into the (fresh or
   * replay-failed) session on the NEW program, divert to the recorded
   * knot, flash the chip. Returns whether it applied. */
  private migrateInto(
    state: SaveState,
    knotPath: string | null,
    transcript: StructuralTranscript | null,
  ): boolean {
    // The live transcript survives a hot reload (RULED 2026-08-30) — and
    // arrives STRUCTURALLY, so re-rendering it against the new program
    // shows the edited prose, the whole point of the format.
    const report = this.loadCheckpoint(state, knotPath, "Reloaded", transcript);
    if (report === null) return false;
    this.reloadedAt = Date.now();
    this.emit();
    return true;
  }

  /**
   * Load a checkpoint (W14/#3307): restore the durable state, divert to
   * the slot's recorded knot (the save format carries no execution
   * position — knot-entry granularity is the honest v1), and reveal.
   * The story-so-far arrives in STRUCTURAL form (RULED 2026-08-30) and
   * is re-rendered against the session's CURRENT program — an edited
   * line's restored row shows the edited text; a non-clean `LoadReport`
   * surfaces as a transcript notice — never a silent load (RULED).
   * Returns the report, `null` without a live session.
   */
  loadCheckpoint(
    state: SaveState,
    knotPath: string | null,
    verb: "Loaded" | "Reloaded" = "Loaded",
    transcript: StructuralTranscript | null = null,
  ): LoadReport | null {
    const session = this.session;
    if (!session) return null;
    let report: LoadReport;
    try {
      // `load_state` is turn-boundary only, and any reveal leaves the
      // session mid-turn (found live: a fresh start's first reveal was
      // enough to refuse the load) — restart to a clean boundary first;
      // the checkpoint replaces the state a restart resets anyway.
      session.restart();
      report = session.loadState(state);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      this.callbacks.notify({
        severity: "error",
        source: "story",
        message: `Load failed: ${msg}`,
      });
      return null;
    }
    this.stopPacedPump();
    this.paused = false;
    this.lastOutcome = null;
    // The caller supplies the story-so-far (a save's stored transcript,
    // or the live one on a hot reload) — dropping it was the RULED-away
    // behavior (2026-08-30). Rendered HERE, against the program this
    // session now runs, not at save time.
    this.transcript =
      transcript === null
        ? []
        : (this.capture(() => session.renderTranscript(transcript)) ?? []).map(
            (line) => transcriptLine(line.text, line.tags, line.source),
          );
    this.choices = [];
    this.status = "running";
    const drops =
      report.anonymous_states_dropped +
      report.unknown_globals.length +
      report.unresolved_renames.length;
    if (drops > 0) {
      const parts: string[] = [];
      if (report.anonymous_states_dropped > 0)
        parts.push(`${report.anonymous_states_dropped} anonymous visit state${report.anonymous_states_dropped === 1 ? "" : "s"} dropped`);
      if (report.unknown_globals.length > 0)
        parts.push(`unknown globals: ${report.unknown_globals.join(", ")}`);
      if (report.unresolved_renames.length > 0)
        parts.push(`unresolved renames: ${report.unresolved_renames.join(", ")}`);
      this.transcript = [
        ...this.transcript,
        transcriptNotice(`${verb} — ${parts.join("; ")}.`),
      ];
    }
    if (knotPath !== null) {
      try {
        // Same dev affordance as play-from-here (M-2b): the recorded
        // knot may be #@private.
        session.setDevVisibilityOverride(true);
        session.goToPath(knotPath);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        this.transcript = [
          ...this.transcript,
          transcriptNotice(`Loaded, but could not divert to ${knotPath}: ${msg}`),
        ];
      }
    }
    this.reveal();
    return report;
  }

  pause(): void {
    // The pause verb (W5/#3298): enter the paused state at the current
    // boundary. Reveals are user-driven in this architecture, so there is
    // never a mid-flight run to interrupt — pausing here makes the NEXT
    // advance a bounded line step and lights the step controls.
    if (this.paused || !this.session) return;
    // Ruled instant flush: pausing mid-paced-run stops the pump NOW —
    // nothing is queued (each tick advanced the VM one line), so
    // stopping the pump is the whole flush.
    this.stopPacedPump();
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

  // ── Peek (ruled 2026-09-03) ───────────────────────────────────────
  //
  // Fork the live session at its exact position (`speculate()` — the F4
  // sandboxed clone-run-drop), run ONE continue call on the fork, report
  // what it hit, free the fork. Never the auto run; externals stay
  // sandboxed (the fork takes the ink fallback body, so a forecast may
  // differ from the real press where an external would have answered —
  // accepted). The path is read BEFORE the advance, exactly as a
  // transcript row's is stamped.

  peekContinue(): PeekResult | null {
    if (!sessionCanContinue(this.status)) return null;
    return this.peek(null);
  }

  peekChoice(index: number): PeekResult | null {
    if (this.status !== "awaiting-choice") return null;
    return this.peek(index);
  }

  private peek(choice: number | null): PeekResult | null {
    const session = this.session;
    if (!session) return null;
    // One visible line is all a peek needs; the budget caps a runaway fork.
    const fork = this.capture(() => session.speculate({ lines: 2 }));
    if (!fork) return null;
    try {
      if (choice !== null) fork.choose(choice);
      const path = fork.currentPath();
      const line = fork.advance();
      const sources: SourceLocation[] = [];
      if (line.type === "text") {
        if (line.source) sources.push(line.source);
      } else if (line.type === "choices") {
        for (const c of line.choices ?? []) if (c.source) sources.push(c.source);
      }
      return { sources, path };
    } catch {
      return null;
    } finally {
      fork.free();
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
      if (this.auto && !this.paused && this.pacedDelayMs > 0) {
        // Paced auto (F13): reveal THIS line now, keep pumping on the
        // timer. Every tick goes through the same single-line road as a
        // manual reveal, so breakpoints bound each step and the
        // execution highlight follows the cadence. `revealOne` emits;
        // return early so the shared tail below doesn't double-emit.
        this.revealOne();
        if (this.status === "running" && !this.paused) {
          this.stopPacedPump();
          this.pacedTimer = setTimeout(() => {
            this.pacedTick();
          }, this.pacedDelayMs);
        }
        return;
      }
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
        let last: SessionLine | undefined;
        if (this.auto) {
          last = this.stepToPause(session);
        } else {
          const path = currentPathOf(session);
          last = session.continueSingle();
          this.appendLines([last], path);
        }
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

  /** One single-line advance — the unit both a manual reveal and each
   *  paced tick share: debug-driven when breakpoints are armed (or
   *  paused), the journaled road otherwise. Refreshes and emits. */
  private revealOne(): void {
    const session = this.session;
    if (!session) {
      this.emit();
      return;
    }
    try {
      if (this.debugDriven()) {
        this.advanceDebug("line", false);
      } else {
        const path = currentPathOf(session);
        const line = session.continueSingle();
        this.appendLines([line], path);
        this.status = statusOfLine(line.type);
        this.choices = line.type === "choices" ? (line.choices ?? []) : [];
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

  private appendLines(lines: SessionLine[], path: string | null = null): void {
    for (const line of lines) {
      const text = line.text.replace(/\n$/, "");
      if (text)
        this.transcript = [
          ...this.transcript,
          transcriptLine(text, line.tags, line.source, path),
        ];
    }
  }

  /** Whether advancement must route through the debug verbs (W5/#3298):
   *  armed breakpoints only ever hit inside the debug loop, and a paused
   *  session resumes through it. */
  private debugDriven(): boolean {
    if (this.paused) return true;
    try {
      // Armed POSITION breakpoints or armed DATA breakpoints (W18) both
      // demand the debug road — the production continue path can hit
      // neither.
      return (
        (this.session?.debugBreakpoints().length ?? 0) > 0 ||
        (this.session?.debugWatchpoints().length ?? 0) > 0
      );
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
    // One line per wasm call on the "run" road too (ruled 2026-09-02), so
    // every row is stamped with the knot/stitch it came from — the path
    // the runtime reports BEFORE the call that delivers it. A run ends at
    // the first stop that is not "landed on the next line" (`step`) —
    // breakpoint, choices, terminal, … — or at the line limit.
    let outcome: DebugRunOutcome;
    const rows: TranscriptLine[] = [];
    let steps = 0;
    do {
      const path = currentPathOf(session);
      outcome = session.debugRunToLine();
      for (const line of outcome.lines) {
        const text = line.text.replace(/\n$/, "");
        if (text) rows.push(transcriptLine(text, line.tags, line.source, path));
      }
      steps += 1;
    } while (kind === "run" && outcome.reason.type === "step" && steps < STEP_LINE_LIMIT);
    this.applyDebugOutcome(outcome, stayPaused, rows);
  }

  /** Fold a debug outcome into the mirrored session state: transcript
   *  delta, paused-ness, status, choices. */
  private applyDebugOutcome(
    outcome: DebugRunOutcome,
    stayPaused: boolean,
    rows?: TranscriptLine[],
  ): void {
    this.lastOutcome = outcome;
    const stamped =
      rows ??
      outcome.lines.flatMap((line) => {
        const text = line.text.replace(/\n$/, "");
        return text ? [transcriptLine(text, line.tags, line.source)] : [];
      });
    if (stamped.length > 0) this.transcript = [...this.transcript, ...stamped];
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

        const chosenSaved = offered.find((c) => c.index === savedChoice);
        if (chosenSaved?.text) allText.push(choiceEcho(chosenSaved.text, chosenSaved));
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
