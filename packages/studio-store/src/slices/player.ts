/**
 * Player slice — story playback state with line-at-a-time reveal.
 *
 * The runtime collects all text into `_pendingLines`. The `revealNext`
 * action pops one line from the buffer into `playerText`. Choices and
 * the end marker only appear once the buffer is drained.
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

  /** Lines waiting to be revealed, plus deferred choices/ended state. */
  _pendingLines: string[];
  _deferredChoices: Choice[];
  _deferredEnded: boolean;

  /** Full choice index log for save/restore. */
  _choiceLog: number[];

  loadStory(bytes: Uint8Array): void;
  chooseOption(index: number): void;
  resetStory(): void;
  /** Reveal the next buffered line (or flush remaining + show choices/end). */
  revealNext(): void;

  /** Player fullscreen mode — hides the editor pane. */
  playerFullscreen: boolean;
  togglePlayerFullscreen(): void;
}

export const createPlayerSlice: StateCreator<StudioState, [], [], PlayerSlice> = (set, get) => ({
  playerText: [],
  playerChoices: [],
  playerEnded: false,
  _runner: null,
  _pendingLines: [],
  _deferredChoices: [],
  _deferredEnded: false,
  _choiceLog: [],
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
      set({
        _runner: runner,
        playerText: [],
        playerChoices: [],
        playerEnded: false,
        _pendingLines: [],
        _deferredChoices: [],
        _deferredEnded: false,
        _choiceLog: [],
      });

      // Check for saved state and replay
      const saved = loadFromStorage();
      if (saved && saved.choiceLog.length > 0) {
        replayChoices(set, get, saved.choiceLog);
      } else {
        advanceStory(set, get);
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set({
        _runner: null,
        playerText: [`Load error: ${msg}`],
        playerChoices: [],
        playerEnded: true,
        _pendingLines: [],
        _deferredChoices: [],
        _deferredEnded: false,
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
        _pendingLines: [],
        _deferredChoices: [],
        _deferredEnded: false,
      }));
      return;
    }

    // Record choice and save
    const newLog = [...get()._choiceLog, index];
    set({ _choiceLog: newLog });
    saveToStorage({ choiceLog: newLog });

    // Append the chosen text as a marker, clear choices, then continue
    set((state) => ({
      playerText: choiceText
        ? [...state.playerText, `> ${choiceText}`]
        : state.playerText,
      playerChoices: [],
    }));
    advanceStory(set, get);
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
      _pendingLines: [],
      _deferredChoices: [],
      _deferredEnded: false,
      _choiceLog: [],
    });
    advanceStory(set, get);
  },

  revealNext() {
    const { _pendingLines, _deferredChoices, _deferredEnded } = get();

    if (_pendingLines.length > 0) {
      // Reveal one line
      const [next, ...rest] = _pendingLines;
      set((state) => ({
        playerText: [...state.playerText, next],
        _pendingLines: rest,
      }));

      // If that was the last pending line, show choices/ended
      if (rest.length === 0) {
        set({
          playerChoices: _deferredChoices,
          playerEnded: _deferredEnded,
          _deferredChoices: [],
          _deferredEnded: false,
        });
      }
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
 * Replay a saved choice log silently — run through the story collecting
 * all text and applying choices without buffering, then show the final
 * state with the current text visible and pending lines buffered.
 */
function replayChoices(set: SetFn, get: GetFn, choiceLog: number[]): void {
  const runner = get()._runner;
  if (!runner) return;

  const allText: string[] = [];
  let choiceIdx = 0;

  // Fast-forward through all saved choices
  while (choiceIdx < choiceLog.length) {
    // Continue until choices or ended
    // eslint-disable-next-line no-constant-condition
    while (true) {
      let result;
      try {
        result = runner.continueStory();
      } catch {
        // Replay failed — start fresh
        clearStorage();
        runner.reset();
        set({ _choiceLog: [] });
        advanceStory(set, get);
        return;
      }

      if (result.text) {
        const lines = result.text.split("\n").filter((l) => l.trim() !== "");
        allText.push(...lines);
      }

      if (result.status === "choices") {
        const savedChoice = choiceLog[choiceIdx];
        const choiceText = result.choices?.find((c) => c.index === savedChoice)?.text;
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
          advanceStory(set, get);
          return;
        }
        choiceIdx++;
        break;
      }

      if (result.status === "ended") {
        // Story ended during replay — show everything
        set({
          playerText: allText,
          playerChoices: [],
          playerEnded: true,
          _pendingLines: [],
          _deferredChoices: [],
          _deferredEnded: false,
          _choiceLog: choiceLog.slice(0, choiceIdx),
        });
        return;
      }
    }
  }

  // All choices replayed — now show the accumulated text and advance
  // to the current position (which will buffer the next batch of lines)
  set({
    playerText: allText,
    _choiceLog: choiceLog,
  });
  advanceStory(set, get);
}

/**
 * Run the continue loop: call continueStory() until we get choices or ended.
 * Buffers all lines into _pendingLines and reveals the first one immediately.
 */
function advanceStory(set: SetFn, get: GetFn): void {
  const runner = get()._runner;
  if (!runner) return;

  const newLines: string[] = [];

  // eslint-disable-next-line no-constant-condition
  while (true) {
    let result;
    try {
      result = runner.continueStory();
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set((state) => ({
        playerText: [...state.playerText, ...newLines, `Runtime error: ${msg}`],
        playerChoices: [],
        playerEnded: true,
        _pendingLines: [],
        _deferredChoices: [],
        _deferredEnded: false,
      }));
      return;
    }

    if (result.text) {
      const lines = result.text.split("\n").filter((l) => l.trim() !== "");
      newLines.push(...lines);
    }

    if (result.status === "continue") {
      continue;
    }

    const choices = result.status === "choices" ? (result.choices ?? []) : [];
    const ended = result.status === "ended";

    if (newLines.length === 0) {
      // Nothing to buffer — show choices/ended immediately
      set({ playerChoices: choices, playerEnded: ended });
    } else {
      // Reveal first line immediately, buffer the rest
      const [first, ...rest] = newLines;
      set((state) => ({
        playerText: [...state.playerText, first],
        _pendingLines: rest,
        _deferredChoices: choices,
        _deferredEnded: ended,
        // If only one line, show choices/ended right away
        ...(rest.length === 0
          ? { playerChoices: choices, playerEnded: ended, _deferredChoices: [], _deferredEnded: false }
          : {}),
      }));
    }
    return;
  }
}
