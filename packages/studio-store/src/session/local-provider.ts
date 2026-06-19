/**
 * Local session provider — the default `SessionProvider` backed by the studio's
 * own wasm `StoryRunner` (docs/live-inspector-spec.md §6.1).
 *
 * Owns the runner lifecycle, the persisted choice log, the silent
 * choice-replay-on-recompile (with divergence truncation), and the pull-based
 * stepping. Every drive operation ends by recomputing the {@link
 * SessionSnapshot} and notifying subscribers; the store mirrors those snapshots
 * into its reactive fields. Behavior is byte-for-byte what the studio did when
 * this logic lived in the session slice — the seam is the only change.
 */

import { StoryRunnerHandle, type ExternalValue } from "@brink-lang/web";
import type { Choice } from "@brink/wasm-types";

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

interface SaveData {
  choiceLog: number[];
}

function saveToStorage(data: SaveData): void {
  try {
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

export class LocalSessionProvider implements SessionProvider {
  readonly kind = "local" as const;
  readonly capabilities: ReadonlySet<SessionCapability> = ALL_CAPABILITIES;

  private runner: StoryRunnerHandle | null;
  private callbacks: ProviderCallbacks;
  private readonly listeners = new Set<(s: SessionSnapshot) => void>();

  // Mirrored snapshot fields.
  private status: SessionStatus = "none";
  private transcript: string[] = [];
  private choices: Choice[] = [];
  private debugState: SessionSnapshot["debugState"] = null;
  private programModel: SessionSnapshot["programModel"] = null;
  private programInkt: string | null = null;
  private programChecksum: string | null = null;

  /** Recorded choice history — persisted for restore + recompile replay. */
  private choiceLog: number[] = [];
  /** Program bytes this session is running — kept so `restart` can re-create. */
  private bytes: Uint8Array | null = null;

  /**
   * Whether to persist + restore the choice log via localStorage. The primary
   * session persists (restore on reload); secondary local sessions (#182) do
   * not — they're transient, isolated playthroughs that must not clobber the
   * primary's save.
   */
  private persist = true;
  /**
   * Optional entry point for the session: navigate here (`go_to_path`) right
   * after load instead of starting at the root — the "play from here as a new
   * session" path (#182). Secondary sessions only.
   */
  private startPath: { path: string; args: ExternalValue[] } | null = null;

  constructor(opts?: {
    callbacks?: ProviderCallbacks;
    /** Adopt an already-live runner (the studio wraps an existing handle; tests). */
    runner?: StoryRunnerHandle;
    /** Status of an adopted runner (default "running"). */
    status?: SessionStatus;
    /** Transcript of an adopted runner (default empty). */
    transcript?: string[];
    /** Pending choices of an adopted runner (default empty). */
    choices?: Choice[];
    /** Persist + restore the choice log (default true; false for secondary sessions). */
    persist?: boolean;
    /** Navigate to this entry point after load instead of the root (#182). */
    startPath?: { path: string; args?: ExternalValue[] };
  }) {
    this.callbacks = opts?.callbacks ?? NOOP_CALLBACKS;
    this.runner = opts?.runner ?? null;
    this.persist = opts?.persist ?? true;
    this.startPath = opts?.startPath
      ? { path: opts.startPath.path, args: opts.startPath.args ?? [] }
      : null;
    if (opts?.runner) {
      this.status = opts.status ?? "running";
      this.transcript = opts.transcript ?? [];
      this.choices = opts.choices ?? [];
    }
  }

  /** Wire the studio services. Called by the session slice at bind time. */
  setCallbacks(callbacks: ProviderCallbacks): void {
    this.callbacks = callbacks;
  }

  /** Whether a live runner exists (drives restart-vs-fresh-start; see slice). */
  hasLiveRunner(): boolean {
    return this.runner !== null;
  }

  /**
   * Spawn a shared-context flow on this session's runner (#200): a concurrent
   * flow of the *same* story that shares globals / visits / rng. Returns a
   * {@link FlowSessionProvider} that drives it, or `null` if there's no live
   * runner. The flow shares this provider's wired callbacks.
   */
  spawnFlow(name: string, path?: string): FlowSessionProvider | null {
    if (!this.runner) return null;
    this.runner.spawnFlow(name, path);
    return new FlowSessionProvider(this.runner, name, this.callbacks);
  }

  /**
   * The recorded choice history (replay state, spec §6.1). Provider-internal —
   * not part of the cross-provider snapshot; exposed read-only for inspection
   * and tests.
   */
  get recordedChoices(): readonly number[] {
    return this.choiceLog;
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
      // Reuse the live runner via in-place hot-reload when one exists: this
      // preserves the replay recording, so the saved choice log replays with
      // faithful externals (query-gated branches reproduce; effect bindings
      // don't re-fire). Fall back to a fresh runner when there's none, or if
      // reload fails (decode/link).
      const prev = this.runner;
      let runner: StoryRunnerHandle;
      if (prev) {
        try {
          prev.reload(bytes);
          runner = prev;
        } catch {
          prev.free();
          runner = new StoryRunnerHandle(bytes);
        }
      } else {
        runner = new StoryRunnerHandle(bytes);
      }
      this.runner = runner;
      this.bytes = bytes;

      // The program inspection is static for the program — capture once on load.
      this.programModel = this.captureProgramModel(runner);
      this.programInkt = this.captureProgramInkt(runner);
      this.programChecksum = this.programModel?.checksum ?? null;

      this.status = "running";
      this.transcript = [];
      this.choices = [];
      this.choiceLog = [];

      // A secondary "play from here" session jumps to its entry point before
      // revealing — no replay (it doesn't persist).
      if (this.startPath) {
        runner.goToPath(this.startPath.path, ...this.startPath.args);
        this.reveal();
        return;
      }

      // Check for saved state and replay; otherwise reveal the first line.
      const saved = this.persist ? loadFromStorage() : null;
      if (saved && saved.choiceLog.length > 0) {
        this.replay(saved.choiceLog);
      } else {
        this.reveal();
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      this.runner = null;
      this.bytes = null;
      this.status = "error";
      this.transcript = [`Load error: ${msg}`];
      this.choices = [];
      this.choiceLog = [];
      this.programModel = null;
      this.programInkt = null;
      this.programChecksum = null;
      this.debugState = null;
      this.callbacks.appendOutput("story", `Load error: ${msg}`);
      this.emit();
    }
  }

  restart(): void {
    if (!this.runner) {
      // No live runner (e.g. a prior load error or a stop) — restart means a
      // fresh start on the bytes this session last ran.
      if (this.bytes) this.start(this.bytes);
      return;
    }
    this.runner.reset();
    if (this.persist) clearStorage();
    this.status = "running";
    this.transcript = [];
    this.choices = [];
    this.choiceLog = [];
    // Re-navigate a "play from here" session to its entry on restart.
    if (this.startPath) this.runner.goToPath(this.startPath.path, ...this.startPath.args);
    this.reveal();
  }

  stop(): void {
    if (this.runner) this.runner.free();
    this.runner = null;
    // Stopping ends the session *intent* — a later `start` is a fresh run, so
    // the persisted choice log goes too.
    if (this.persist) clearStorage();
    this.choiceLog = [];
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
    const runner = this.runner;
    if (!runner) return;

    const choiceText = this.choices.find((c) => c.index === index)?.text;

    try {
      runner.choose(index);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      this.status = "error";
      this.transcript = [...this.transcript, `Choose error: ${msg}`];
      this.choices = [];
      this.callbacks.appendOutput("story", `Choose error: ${msg}`);
      this.emit();
      return;
    }

    // Record choice and save (secondary sessions don't persist — §182).
    this.choiceLog = [...this.choiceLog, index];
    if (this.persist) saveToStorage({ choiceLog: this.choiceLog });

    // Append the chosen text as a marker, clear choices.
    if (choiceText) this.transcript = [...this.transcript, `> ${choiceText}`];
    this.choices = [];

    // Reveal first line of the next section (emits).
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
    if (this.runner) this.runner.free();
    this.runner = null;
    this.bytes = null;
    this.listeners.clear();
    this.status = "none";
    this.transcript = [];
    this.choices = [];
    this.debugState = null;
    this.programModel = null;
    this.programInkt = null;
    this.programChecksum = null;
    this.choiceLog = [];
  }

  // ── Internals ─────────────────────────────────────────────────────

  private captureProgramModel(runner: StoryRunnerHandle): SessionSnapshot["programModel"] {
    try {
      return runner.programModel();
    } catch {
      return null;
    }
  }

  private captureProgramInkt(runner: StoryRunnerHandle): string | null {
    try {
      return runner.programInkt();
    } catch {
      return null;
    }
  }

  /** Reveal the next line from the runtime (or surface choices/end). Emits. */
  private reveal(): void {
    const runner = this.runner;
    if (!runner) {
      this.emit();
      return;
    }

    try {
      const line = runner.continueSingle();
      const text = line.text.replace(/\n$/, "");
      if (text) this.transcript = [...this.transcript, text];
      this.choices = line.type === "choices" ? (line.choices ?? []) : [];
      this.status = statusOfLine(line.type);
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

  private refreshDebug(): void {
    const runner = this.runner;
    if (!runner) {
      this.debugState = null;
      return;
    }
    try {
      this.debugState = runner.debugSnapshot();
    } catch {
      // The runner can be mid-teardown or in an error state — never let the
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
   * Replay a recorded choice log silently — run through the story collecting
   * all text and applying choices, then leave the session at the final state.
   *
   * Recompile-while-running (spec §7.6): if the program changed and a recorded
   * choice can no longer be applied — its index is no longer offered, the story
   * ends or dead-ends (`-> DONE`) before reaching it, or the runtime errors —
   * the replay *truncates the history at the divergence point*, keeps the
   * session at the position it reached, and raises a divergence notification.
   *
   * When the runner holds a recording (the player played before a hot-reload),
   * externals are served from it during the silent re-walk *and* the
   * current-page reveal, so query-gated branches reproduce and effect bindings
   * don't re-fire. On a fresh load the recording is empty, so replay runs live.
   */
  private replay(choiceLog: number[]): void {
    const runner = this.runner;
    if (!runner) return;

    const useRecording = runner.hasRecording();
    if (useRecording) runner.beginReplay();
    try {
      this.replayWalk(runner, choiceLog);
    } finally {
      if (useRecording) runner.endReplay();
    }
  }

  private replayWalk(runner: StoryRunnerHandle, choiceLog: number[]): void {
    const allText: string[] = [];
    let choiceIdx = 0;

    // Truncate the recorded history at the divergence point: keep the prefix
    // that was consumed, persist it, and notify (spec §7.5 warning).
    const truncateLog = (): void => {
      const kept = choiceLog.slice(0, choiceIdx);
      if (kept.length > 0) {
        saveToStorage({ choiceLog: kept });
      } else {
        clearStorage();
      }
      this.choiceLog = kept;
      this.notifyDiverged();
    };

    // Hard backstop only: something is pathological enough that the VM position
    // can't be trusted — reset to a fresh run.
    const bailToFresh = (): void => {
      clearStorage();
      runner.reset();
      this.choiceLog = [];
      this.notifyDiverged();
      this.reveal();
    };

    // Each pass must consume exactly one saved choice. Cap iterations at the
    // number of saved choices (+1 margin) so a story that dead-ends on DONE
    // before reaching the next saved choice can't spin forever (it would lock
    // the UI thread). The `consumedChoice` check below is the precise guard;
    // this cap is a hard backstop.
    let budget = choiceLog.length + 1;
    while (choiceIdx < choiceLog.length) {
      if (budget-- <= 0) {
        bailToFresh();
        return;
      }

      let lines;
      try {
        lines = runner.continueStory();
      } catch (e) {
        // The program errored before reaching the next recorded choice.
        const msg = e instanceof Error ? e.message : String(e);
        truncateLog();
        this.transcript = [...allText, `Runtime error: ${msg}`];
        this.choices = [];
        this.status = "error";
        this.callbacks.appendOutput("story", `Runtime error: ${msg}`);
        this.refreshDebug();
        this.emit();
        return;
      }

      let consumedChoice = false;
      let lastType = "done";
      for (const line of lines) {
        const text = line.text.replace(/\n$/, "");
        if (text) allText.push(text);
        lastType = line.type;

        if (line.type === "choices") {
          const savedChoice = choiceLog[choiceIdx];
          const offered = line.choices ?? [];
          let chose = false;
          if (
            savedChoice !== undefined &&
            offered.some((c) => c.index === savedChoice)
          ) {
            try {
              runner.choose(savedChoice);
              chose = true;
            } catch {
              chose = false;
            }
          }

          if (!chose) {
            // The recorded index is no longer valid — divergence. Stay at this
            // choice point and let the user pick from what is offered now.
            truncateLog();
            this.transcript = allText;
            this.choices = offered;
            this.status = "awaiting-choice";
            this.refreshDebug();
            this.emit();
            return;
          }

          const choiceText = offered.find((c) => c.index === savedChoice)?.text;
          if (choiceText) allText.push(`> ${choiceText}`);
          choiceIdx++;
          consumedChoice = true;
          break;
        }

        if (line.type === "end") {
          // The story now ends before consuming the full history — divergence
          // (the remaining recorded choices are unreachable). Truncate and show.
          truncateLog();
          this.transcript = allText;
          this.choices = [];
          this.status = "ended";
          this.refreshDebug();
          this.emit();
          return;
        }
      }

      // The pass produced no choice to consume and didn't end (it reached a
      // `-> DONE` dead-end). The next recorded choice is unreachable —
      // divergence. Truncate and stay at the turn boundary rather than calling
      // continueStory() forever.
      if (!consumedChoice) {
        truncateLog();
        this.transcript = allText;
        this.choices = [];
        this.status = statusOfLine(lastType);
        this.refreshDebug();
        this.emit();
        return;
      }
    }

    // All choices replayed — show accumulated text and reveal the next line.
    this.transcript = allText;
    this.choiceLog = choiceLog;
    this.reveal();
  }
}
