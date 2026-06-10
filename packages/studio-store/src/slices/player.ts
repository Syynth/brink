/**
 * Player slice — story playback state with line-at-a-time reveal.
 *
 * Uses the `continueSingle()` API: each call to `revealNext` fetches
 * one real runtime step. No client-side buffering needed.
 *
 * Choice log: every choice index is recorded in `_choiceLog`. On
 * `loadStory`, if a saved log exists in localStorage, the story is
 * replayed silently to restore the previous state.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { Choice, DebugState, ProgramModel } from "@brink/wasm-types";
import { StoryRunnerHandle } from "@brink/wasm";

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

export interface PlayerSlice {
  playerText: string[];
  playerChoices: Choice[];
  playerEnded: boolean;
  /**
   * Whether the last revealed line was a `text` line — i.e. the runtime
   * has more output to reveal before reaching choices or the end. Drives
   * the "Continue" button in the player.
   */
  playerCanContinue: boolean;
  _runner: StoryRunnerHandle | null;

  /** Full choice index log for save/restore. */
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
   * captured once when a story loads (static). `null` until loaded.
   */
  programModel: ProgramModel | null;
  /** The compiled program as `.inkt` text — the Program Explorer's raw toggle. */
  programInkt: string | null;

  loadStory(bytes: Uint8Array): void;
  chooseOption(index: number): void;
  resetStory(): void;
  /** Reveal the next line from the runtime (or show choices/end). */
  revealNext(): void;
  /** Refresh `debugState` from the current runner (no-op without one). */
  _refreshDebugState(): void;
  /** Free the current story runner's wasm memory (call on teardown). */
  disposePlayer(): void;

  /** Player fullscreen mode — hides the editor pane. */
  playerFullscreen: boolean;
  togglePlayerFullscreen(): void;
}

export const createPlayerSlice: StateCreator<StudioState, [], [], PlayerSlice> = (set, get) => ({
  playerText: [],
  playerChoices: [],
  playerEnded: false,
  playerCanContinue: false,
  _runner: null,
  _choiceLog: [],
  debugState: null,
  prevDebugState: null,
  programModel: null,
  programInkt: null,
  playerFullscreen: false,

  togglePlayerFullscreen() {
    set((state) => ({ playerFullscreen: !state.playerFullscreen }));
  },

  loadStory(bytes) {
    const prev = get()._runner;
    if (prev) {
      prev.free();
    }

    try {
      const runner = new StoryRunnerHandle(bytes);
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
        playerText: [],
        playerChoices: [],
        playerEnded: false,
        playerCanContinue: false,
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
        playerText: [`Load error: ${msg}`],
        playerChoices: [],
        playerEnded: true,
        playerCanContinue: false,
        _choiceLog: [],
        programModel: null,
        programInkt: null,
      });
    }
  },

  chooseOption(index) {
    const runner = get()._runner;
    if (!runner) return;

    const choiceText = get().playerChoices.find((c) => c.index === index)?.text;

    try {
      runner.choose(index);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set((state) => ({
        playerText: [...state.playerText, `Choose error: ${msg}`],
        playerChoices: [],
        playerEnded: true,
        playerCanContinue: false,
      }));
      return;
    }

    // Record choice and save
    const newLog = [...get()._choiceLog, index];
    set({ _choiceLog: newLog });
    saveToStorage({ choiceLog: newLog });

    // Append the chosen text as a marker, clear choices
    set((state) => ({
      playerText: choiceText
        ? [...state.playerText, `> ${choiceText}`]
        : state.playerText,
      playerChoices: [],
    }));

    // Reveal first line of next section
    get().revealNext();
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

  disposePlayer() {
    const runner = get()._runner;
    if (runner) runner.free();
    set({
      _runner: null,
      debugState: null,
      prevDebugState: null,
      programModel: null,
      programInkt: null,
    });
  },

  resetStory() {
    const runner = get()._runner;
    if (!runner) return;
    runner.reset();
    clearStorage();
    set({
      playerText: [],
      playerChoices: [],
      playerEnded: false,
      playerCanContinue: false,
      _choiceLog: [],
    });
    get().revealNext();
  },

  revealNext() {
    const runner = get()._runner;
    if (!runner) return;

    try {
      const line = runner.continueSingle();
      const text = line.text.replace(/\n$/, "");
      set((state) => ({
        playerText: text ? [...state.playerText, text] : state.playerText,
        playerChoices: line.type === "choices" ? (line.choices ?? []) : [],
        playerEnded: line.type === "end",
        // `done` (ink `-> DONE`) is a turn boundary, not the end — keep the
        // Continue button so the player can resume past it, like a `text` line.
        playerCanContinue: line.type === "text" || line.type === "done",
      }));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set((state) => ({
        playerText: [...state.playerText, `Runtime error: ${msg}`],
        playerChoices: [],
        playerEnded: true,
        playerCanContinue: false,
      }));
    }

    // Every visible advance funnels through here (load, reset, choose, and the
    // tail of replay), so refreshing the State View snapshot here covers them all.
    get()._refreshDebugState();
  },
});

// ── Helpers ──────────────────────────────────────────────────────────

type SetFn = {
  (partial: Partial<StudioState>): void;
  (updater: (state: StudioState) => Partial<StudioState>): void;
};
type GetFn = () => StudioState;

/**
 * Replay a saved choice log silently — run through the story using the
 * bulk `continueStory()` API, collecting all text and applying choices,
 * then show the final state with text visible.
 */
export function replayChoices(set: SetFn, get: GetFn, choiceLog: number[]): void {
  const runner = get()._runner;
  if (!runner) return;

  const allText: string[] = [];
  let choiceIdx = 0;

  // The saved log no longer matches the current story — discard it and start
  // the story fresh.
  const bailToFresh = (): void => {
    clearStorage();
    runner.reset();
    set({ _choiceLog: [] });
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
    } catch {
      bailToFresh();
      return;
    }

    let consumedChoice = false;
    for (const line of lines) {
      const text = line.text.replace(/\n$/, "");
      if (text) {
        allText.push(text);
      }

      if (line.type === "choices") {
        const savedChoice = choiceLog[choiceIdx];
        const choiceText = line.choices?.find((c) => c.index === savedChoice)?.text;
        if (choiceText) {
          allText.push(`> ${choiceText}`);
        }

        try {
          runner.choose(savedChoice);
        } catch {
          bailToFresh();
          return;
        }
        choiceIdx++;
        consumedChoice = true;
        break;
      }

      if (line.type === "end") {
        // Story ended during replay — show everything
        set({
          playerText: allText,
          playerChoices: [],
          playerEnded: true,
          playerCanContinue: false,
          _choiceLog: choiceLog.slice(0, choiceIdx),
        });
        get()._refreshDebugState();
        return;
      }
    }

    // The pass produced no choice to consume and didn't end (e.g. it reached a
    // `-> DONE` dead-end). The saved choice can't be replayed, so the log is
    // stale — bail rather than calling continueStory() forever.
    if (!consumedChoice) {
      bailToFresh();
      return;
    }
  }

  // All choices replayed — show accumulated text and reveal next line
  set({
    playerText: allText,
    _choiceLog: choiceLog,
  });
  get().revealNext();
}
