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
import type { Choice } from "@brink/wasm-types";
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
  _runner: StoryRunnerHandle | null;

  /** Full choice index log for save/restore. */
  _choiceLog: number[];

  loadStory(bytes: Uint8Array): void;
  chooseOption(index: number): void;
  resetStory(): void;
  /** Reveal the next line from the runtime (or show choices/end). */
  revealNext(): void;

  /** Player fullscreen mode — hides the editor pane. */
  playerFullscreen: boolean;
  togglePlayerFullscreen(): void;

  /** Whether the player pane is visible. */
  playerVisible: boolean;
  togglePlayerVisible(): void;
}

export const createPlayerSlice: StateCreator<StudioState, [], [], PlayerSlice> = (set, get) => ({
  playerText: [],
  playerChoices: [],
  playerEnded: false,
  _runner: null,
  _choiceLog: [],
  playerFullscreen: false,
  playerVisible: true,

  togglePlayerFullscreen() {
    set((state) => ({ playerFullscreen: !state.playerFullscreen }));
  },

  togglePlayerVisible() {
    set((state) => ({ playerVisible: !state.playerVisible }));
  },

  loadStory(bytes) {
    const prev = get()._runner;
    if (prev) {
      prev.free();
    }

    try {
      const runner = new StoryRunnerHandle(bytes);
      set({
        _runner: runner,
        playerText: [],
        playerChoices: [],
        playerEnded: false,
        _choiceLog: [],
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
        _choiceLog: [],
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

  resetStory() {
    const runner = get()._runner;
    if (!runner) return;
    runner.reset();
    clearStorage();
    set({
      playerText: [],
      playerChoices: [],
      playerEnded: false,
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
      }));
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set((state) => ({
        playerText: [...state.playerText, `Runtime error: ${msg}`],
        playerChoices: [],
        playerEnded: true,
      }));
    }
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
function replayChoices(set: SetFn, get: GetFn, choiceLog: number[]): void {
  const runner = get()._runner;
  if (!runner) return;

  const allText: string[] = [];
  let choiceIdx = 0;

  // Fast-forward through all saved choices
  while (choiceIdx < choiceLog.length) {
    let lines;
    try {
      lines = runner.continueStory();
    } catch {
      // Replay failed — start fresh
      clearStorage();
      runner.reset();
      set({ _choiceLog: [] });
      get().revealNext();
      return;
    }

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
          // Invalid choice — start fresh
          clearStorage();
          runner.reset();
          set({ _choiceLog: [] });
          get().revealNext();
          return;
        }
        choiceIdx++;
        break;
      }

      if (line.type === "end") {
        // Story ended during replay — show everything
        set({
          playerText: allText,
          playerChoices: [],
          playerEnded: true,
          _choiceLog: choiceLog.slice(0, choiceIdx),
        });
        return;
      }
    }
  }

  // All choices replayed — show accumulated text and reveal next line
  set({
    playerText: allText,
    _choiceLog: choiceLog,
  });
  get().revealNext();
}
