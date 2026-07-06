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
 * `debugSnapshot().pending_choices`, index-zipped in order — `DebugChoice`
 * carries text/target but not `index`; the Rust `Choice.index` is exactly the
 * post-filter enumeration position over the same list (`story.rs`'s
 * `pending_choices` builder and `debug_snapshot`'s builder apply the same
 * `!is_invisible_default` filter in the same order), so this is a safe,
 * documented reconstruction, not a guess. `tags` isn't tracked per
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
import type { Choice, ReplayOutcome, SessionJournal, SessionLine } from "@brink/wasm-types";

import { FlowSessionProvider } from "./flow-provider.js";

import {
  ALL_CAPABILITIES,
  sessionCanContinue,
  statusOfLine,
  type ProviderCallbacks,
  type SessionCapability,
  type SessionProvider,
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

/** Zip `debugSnapshot().pending_choices` into `Choice[]` — see the
 * "Post-restore/reload transcript" doc comment above for why this is a safe
 * reconstruction rather than a guess. */
function choicesFromDebugState(
  debugState: SessionSnapshot["debugState"],
): Choice[] {
  if (!debugState) return [];
  return debugState.pending_choices.map((c, index) => ({
    index,
    text: c.text,
    tags: [],
  }));
}

export class LocalSessionProvider implements SessionProvider {
  readonly kind = "local" as const;
  readonly capabilities: ReadonlySet<SessionCapability> = ALL_CAPABILITIES;

  private session: StorySessionHandle | null;
  private callbacks: ProviderCallbacks;
  private readonly listeners = new Set<(s: SessionSnapshot) => void>();
  /** Unsubscribe from the bound session's `onJournalDirty` hook. */
  private journalUnsub: (() => void) | null = null;

  // Mirrored snapshot fields.
  private status: SessionStatus = "none";
  private transcript: string[] = [];
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
      this.transcript = opts.transcript ?? [];
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
        outcome = prev.reload(bytes);
        session = prev;
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
      // revealing — no persisted restore (it doesn't persist).
      if (this.startPath) {
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
      this.session = null;
      this.bytes = null;
      this.status = "error";
      this.transcript = [`Load error: ${msg}`];
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
    // Re-navigate a "play from here" session to its entry on restart.
    if (this.startPath) this.session.goToPath(this.startPath.path, ...this.startPath.args);
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
      this.transcript = [...this.transcript, `Choose error: ${msg}`];
      this.choices = [];
      this.callbacks.appendOutput("story", `Choose error: ${msg}`);
      this.emit();
      return;
    }

    // Append the chosen text as a marker, clear choices.
    if (choiceText) this.transcript = [...this.transcript, `> ${choiceText}`];
    this.choices = [];

    // Reveal the next section (emits). The journal-dirty hook handles
    // persistence — no bespoke save call here.
    this.reveal();
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

  /** Reveal the next line from the runtime (or surface choices/end). Emits. */
  private reveal(): void {
    const session = this.session;
    if (!session) {
      this.emit();
      return;
    }

    try {
      const lines = session.continueToPause();
      this.appendLines(lines);
      const last = lines.at(-1);
      this.status = last ? statusOfLine(last.type) : this.status;
      this.choices = last?.type === "choices" ? (last.choices ?? []) : [];
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      this.status = "error";
      this.transcript = [...this.transcript, `Runtime error: ${msg}`];
      this.choices = [];
      this.callbacks.appendOutput("story", `Runtime error: ${msg}`);
    }

    this.refreshDebug();
    this.emit();
  }

  private appendLines(lines: SessionLine[]): void {
    for (const line of lines) {
      const text = line.text.replace(/\n$/, "");
      if (text) this.transcript = [...this.transcript, text];
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
            `Runtime error: ${outcome.reason.message}`,
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

    const allText: string[] = [];
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
        allText.push(`Runtime error: ${msg}`);
        diverge();
        return;
      }

      for (const line of lines) {
        const text = line.text.replace(/\n$/, "");
        if (text) allText.push(text);
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
        if (choiceText) allText.push(`> ${choiceText}`);
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
