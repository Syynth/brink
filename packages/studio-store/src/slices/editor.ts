/**
 * Editor slice — cursor position and current line metadata.
 *
 * Updated at high frequency by CM6 editor callbacks.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { LineInfo, KeyHint } from "../types.js";

export interface EditorSlice {
  cursor: { line: number; col: number };
  currentLineInfo: LineInfo | null;
  currentLineHints: KeyHint[];

  setCursor(line: number, col: number): void;
  setLineInfo(info: LineInfo | null, hints: KeyHint[]): void;
}

export const createEditorSlice: StateCreator<StudioState, [], [], EditorSlice> = (set) => ({
  cursor: { line: 1, col: 1 },
  currentLineInfo: null,
  currentLineHints: [],

  setCursor(line, col) {
    set({ cursor: { line, col } });
  },

  setLineInfo(info, hints) {
    set({ currentLineInfo: info, currentLineHints: hints });
  },
});
