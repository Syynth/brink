/**
 * Session slice — the story session as a first-class model
 * (docs/studio-shell-spec.md §7.6).
 *
 * Owns the studio's handle on a live VM instance: program identity (the
 * loaded story bytes), the `StoryRunnerHandle`, the append-only transcript
 * for the current run, the name-resolved debug snapshot, the recorded choice
 * history, and the session status:
 *
 *   none → running → awaiting-choice → (done | ended | error)
 *
 * Lifecycle belongs to commands (`story.start` / `story.restart` /
 * `story.stop` / `story.choose` / `story.continue`, registered at the app
 * boundary) — these slice actions are the implementation those commands call.
 * No view mutates the session directly. Player *UI* state (fullscreen, …)
 * stays in the player slice — different lifetime, different owner.
 *
 * Choice log: every choice index is recorded in `_choiceLog` and persisted
 * to localStorage. On `startSession` (including the recompile auto-start),
 * a saved log is replayed silently to restore the previous position. If the
 * program changed and a recorded choice no longer applies, the replay
 * truncates at the divergence point and raises a warning through the injected
 * notifier (`_notify` → shell notification service, spec §7.5/§7.6).
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { Choice, DebugState, ProgramModel } from "@brink/wasm-types";
import { StoryRunnerHandle } from "@brink-lang/web";

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

// ── Status ──────────────────────────────────────────────────────────

/** Story session status (spec §7.6). */
export type SessionStatus =
  | "none"
  | "running"
  | "awaiting-choice"
  | "done"
  | "ended"
  | "error";

/** Map a runtime line type to the session status it leaves us in. */
function statusOfLine(type: string): SessionStatus {
  switch (type) {
    case "choices":
      return "awaiting-choice";
    case "end":
      return "ended";
    // `-> DONE` is a turn boundary, not the end — the player can still
    // continue past it (#6), which `sessionCanContinue` reflects.
    case "done":
      return "done";
    default:
      return "running";
  }
}

/**
 * Whether the session can advance another line (drives the player's
 * "Continue" button and the `story.continue` command's `when`). True
 * mid-flow and at a `-> DONE` turn boundary; false at choices/end/error.
 */
export function sessionCanContinue(status: SessionStatus): boolean {
  return status === "running" || status === "done";
}

// ── Slice ───────────────────────────────────────────────────────────

export interface SessionSlice {
  /** Lifecycle status — "none" means no session exists (placeholder UIs). */
  sessionStatus: SessionStatus;
  /** Append-only transcript for the current run (cleared on restart). */
  sessionText: string[];
  /** Pending choices; non-empty only when status is "awaiting-choice". */
  sessionChoices: Choice[];

  /** The live VM handle. Non-reactive ref by convention (`_` prefix). */
  _runner: StoryRunnerHandle | null;
  /**
   * Program identity: the bytes this session is running. Kept across
   * `stopSession` so `story.start` can begin a fresh session on the same
   * program even when the latest compile failed (which nulls `storyBytes`).
   */
  _sessionBytes: Uint8Array | null;
  /** Recorded choice history — persisted for restore + recompile replay. */
  _choiceLog: number[];

  /**
   * Structured, name-resolved runtime snapshot for the State View — refreshed
   * whenever the story advances. `null` until a story is running.
   */
  debugState: DebugState | null;
  /**
   * The snapshot from *before* the latest advance, so the State View can
   * highlight what changed this step. `null` on the first snapshot.
   */
  prevDebugState: DebugState | null;
  /**
   * Structured model of the compiled program for the Program Explorer —
   * captured once when a program loads (static). The Program Explorer is
   * compile-bound (spec §4), so this survives `stopSession`.
   */
  programModel: ProgramModel | null;
  /**
   * The compiled program as `.inkt` text — rendered by the read-only
   * Compiled Output document (#91). Compile-bound like `programModel`.
   */
  programInkt: string | null;

  /**
   * Start (or restart) a session on `bytes`. The single code path for both
   * the `story.start` command and the auto-start on successful compile —
   * replays any persisted choice log to restore position.
   */
  startSession(bytes: Uint8Array): void;
  /** Restart the current session's program from the beginning, fresh log. */
  restartSession(): void;
  /** End the session: free the runner, clear the log, status → "none". */
  stopSession(): void;
  /** Apply a choice by index (status must be "awaiting-choice"). */
  chooseOption(index: number): void;
  /** Reveal the next line from the runtime (or surface choices/end). */
  revealNext(): void;
  /** Refresh `debugState` from the current runner (no-op without one). */
  _refreshDebugState(): void;
  /** Free the runner and program inspection state (app teardown). */
  disposeSession(): void;
}

export const createSessionSlice: StateCreator<StudioState, [], [], SessionSlice> = (
  set,
  get,
) => ({
  sessionStatus: "none",
  sessionText: [],
  sessionChoices: [],
  _runner: null,
  _sessionBytes: null,
  _choiceLog: [],
  debugState: null,
  prevDebugState: null,
  programModel: null,
  programInkt: null,

  startSession(bytes) {
    const prev = get()._runner;

    try {
      // Reuse the live runner via in-place hot-reload when one exists: this
      // preserves the replay recording, so the saved choice log replays with
      // faithful externals (query-gated branches reproduce; effect bindings
      // don't re-fire). Fall back to a fresh runner when there's none, or if
      // reload fails (decode/link).
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
      // The program inspection is static for the program — capture once on load.
      let programModel: ProgramModel | null = null;
      let programInkt: string | null = null;
      try {
        programModel = runner.programModel();
      } catch {
        programModel = null;
      }
      try {
        programInkt = runner.programInkt();
      } catch {
        programInkt = null;
      }
      set({
        _runner: runner,
        _sessionBytes: bytes,
        sessionStatus: "running",
        sessionText: [],
        sessionChoices: [],
        _choiceLog: [],
        programModel,
        programInkt,
      });

      // Check for saved state and replay
      const saved = loadFromStorage();
      if (saved && saved.choiceLog.length > 0) {
        replayChoices(set, get, saved.choiceLog);
      } else {
        get().revealNext();
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set({
        _runner: null,
        _sessionBytes: null,
        sessionStatus: "error",
        sessionText: [`Load error: ${msg}`],
        sessionChoices: [],
        _choiceLog: [],
        programModel: null,
        programInkt: null,
      });
      get().appendOutput("story", `Load error: ${msg}`);
    }
  },

  restartSession() {
    const runner = get()._runner;
    if (!runner) return;
    runner.reset();
    clearStorage();
    set({
      sessionStatus: "running",
      sessionText: [],
      sessionChoices: [],
      _choiceLog: [],
    });
    get().revealNext();
  },

  stopSession() {
    const runner = get()._runner;
    if (runner) runner.free();
    // Stopping ends the session *intent* — a later `story.start` is a fresh
    // run, so the persisted choice log goes too.
    clearStorage();
    set({
      _runner: null,
      sessionStatus: "none",
      sessionText: [],
      sessionChoices: [],
      _choiceLog: [],
      debugState: null,
      prevDebugState: null,
      // _sessionBytes / programModel / programInkt are kept — the program
      // identity outlives the run (story.start restarts it; the Program
      // Explorer is compile-bound, not session-bound).
    });
  },

  chooseOption(index) {
    const runner = get()._runner;
    if (!runner) return;

    const choiceText = get().sessionChoices.find((c) => c.index === index)?.text;

    try {
      runner.choose(index);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set((state) => ({
        sessionStatus: "error",
        sessionText: [...state.sessionText, `Choose error: ${msg}`],
        sessionChoices: [],
      }));
      get().appendOutput("story", `Choose error: ${msg}`);
      return;
    }

    // Record choice and save
    const newLog = [...get()._choiceLog, index];
    set({ _choiceLog: newLog });
    saveToStorage({ choiceLog: newLog });

    // Append the chosen text as a marker, clear choices
    set((state) => ({
      sessionText: choiceText
        ? [...state.sessionText, `> ${choiceText}`]
        : state.sessionText,
      sessionChoices: [],
    }));

    // Reveal first line of next section
    get().revealNext();
  },

  revealNext() {
    const runner = get()._runner;
    if (!runner) return;

    try {
      const line = runner.continueSingle();
      const text = line.text.replace(/\n$/, "");
      set((state) => ({
        sessionText: text ? [...state.sessionText, text] : state.sessionText,
        sessionChoices: line.type === "choices" ? (line.choices ?? []) : [],
        sessionStatus: statusOfLine(line.type),
      }));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set((state) => ({
        sessionStatus: "error",
        sessionText: [...state.sessionText, `Runtime error: ${msg}`],
        sessionChoices: [],
      }));
      get().appendOutput("story", `Runtime error: ${msg}`);
    }

    // Every visible advance funnels through here (start, restart, choose, and
    // the tail of replay), so refreshing the State View snapshot here covers
    // them all.
    get()._refreshDebugState();
  },

  _refreshDebugState() {
    const runner = get()._runner;
    if (!runner) {
      set({ debugState: null, prevDebugState: null });
      return;
    }
    try {
      const next = runner.debugSnapshot();
      // Carry the prior snapshot forward so the View can diff this step.
      set((s) => ({ prevDebugState: s.debugState, debugState: next }));
    } catch {
      // The runner can be mid-teardown or in an error state — never let the
      // debug snapshot throw into the UI.
      set({ debugState: null, prevDebugState: null });
    }
  },

  disposeSession() {
    const runner = get()._runner;
    if (runner) runner.free();
    set({
      _runner: null,
      _sessionBytes: null,
      sessionStatus: "none",
      debugState: null,
      prevDebugState: null,
      programModel: null,
      programInkt: null,
    });
  },
});

// ── Replay ──────────────────────────────────────────────────────────

type SetFn = {
  (partial: Partial<StudioState>): void;
  (updater: (state: StudioState) => Partial<StudioState>): void;
};
type GetFn = () => StudioState;

/**
 * The divergence notification (spec §7.6). Raised as a "warning" from source
 * "story" through the notification service (spec §7.5).
 */
export const REPLAY_DIVERGED_MESSAGE =
  "Story changed — replay diverged; choice history truncated.";

/** Raise the divergence warning through the injected notifier (spec §7.5). */
function notifyDiverged(get: GetFn): void {
  get()._notify?.({
    severity: "warning",
    source: "story",
    message: REPLAY_DIVERGED_MESSAGE,
  });
}

/**
 * Replay a recorded choice log silently — run through the story using the
 * bulk `continueStory()` API, collecting all text and applying choices,
 * then show the final state with text visible.
 *
 * Recompile-while-running (spec §7.6): if the program changed and a recorded
 * choice can no longer be applied — its index is no longer offered, the story
 * ends or dead-ends (`-> DONE`) before reaching it, or the runtime errors —
 * the replay *truncates the history at the divergence point*, keeps the
 * session at the position it reached, and raises a divergence notification.
 */
export function replayChoices(set: SetFn, get: GetFn, choiceLog: number[]): void {
  const runner = get()._runner;
  if (!runner) return;

  // When the runner holds a recording (the player played before a hot-reload),
  // serve externals from it during the silent re-walk *and* the current-page
  // reveal, so query-gated branches reproduce and effect bindings don't
  // re-fire. On a fresh load the recording is empty, so replay runs live
  // (today's behavior). end_replay always runs, even on a divergence return.
  const useRecording = runner.hasRecording();
  if (useRecording) runner.beginReplay();
  try {
    replayChoicesWalk(set, get, runner, choiceLog);
  } finally {
    if (useRecording) runner.endReplay();
  }
}

function replayChoicesWalk(
  set: SetFn,
  get: GetFn,
  runner: StoryRunnerHandle,
  choiceLog: number[],
): void {
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
    set({ _choiceLog: kept });
    notifyDiverged(get);
  };

  // Hard backstop only (see the budget note below): something is pathological
  // enough that the VM position can't be trusted — reset to a fresh run.
  const bailToFresh = (): void => {
    clearStorage();
    runner.reset();
    set({ _choiceLog: [] });
    notifyDiverged(get);
    get().revealNext();
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
      set({
        sessionText: [...allText, `Runtime error: ${msg}`],
        sessionChoices: [],
        sessionStatus: "error",
      });
      get().appendOutput("story", `Runtime error: ${msg}`);
      get()._refreshDebugState();
      return;
    }

    let consumedChoice = false;
    let lastType = "done";
    for (const line of lines) {
      const text = line.text.replace(/\n$/, "");
      if (text) {
        allText.push(text);
      }
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
          set({
            sessionText: allText,
            sessionChoices: offered,
            sessionStatus: "awaiting-choice",
          });
          get()._refreshDebugState();
          return;
        }

        const choiceText = offered.find((c) => c.index === savedChoice)?.text;
        if (choiceText) {
          allText.push(`> ${choiceText}`);
        }
        choiceIdx++;
        consumedChoice = true;
        break;
      }

      if (line.type === "end") {
        // The story now ends before consuming the full history — divergence
        // (the remaining recorded choices are unreachable). Truncate and show.
        truncateLog();
        set({
          sessionText: allText,
          sessionChoices: [],
          sessionStatus: "ended",
        });
        get()._refreshDebugState();
        return;
      }
    }

    // The pass produced no choice to consume and didn't end (it reached a
    // `-> DONE` dead-end). The next recorded choice is unreachable —
    // divergence. Truncate and stay at the turn boundary rather than calling
    // continueStory() forever.
    if (!consumedChoice) {
      truncateLog();
      set({
        sessionText: allText,
        sessionChoices: [],
        sessionStatus: statusOfLine(lastType),
      });
      get()._refreshDebugState();
      return;
    }
  }

  // All choices replayed — show accumulated text and reveal next line
  set({
    sessionText: allText,
    _choiceLog: choiceLog,
  });
  get().revealNext();
}
