/**
 * Editor slice — cursor position and current line metadata.
 *
 * Updated at high frequency by CM6 editor callbacks.
 */

import type { StateCreator } from "zustand";
import type { FormGlyphMode } from "@brink/ink-editor";
import type { StudioState } from "../index.js";
import type { LineInfo, KeyHint } from "../types.js";

export interface EditorSlice {
  cursor: { line: number; col: number };
  currentLineInfo: LineInfo | null;
  currentLineHints: KeyHint[];
  /** Inline argument-form glyph mode (Settings; applied live to all editors). */
  formGlyph: FormGlyphMode;

  setCursor(line: number, col: number): void;
  setLineInfo(info: LineInfo | null, hints: KeyHint[]): void;
  setFormGlyph(mode: FormGlyphMode): void;
}

export const createEditorSlice: StateCreator<StudioState, [], [], EditorSlice> = (set, get) => ({
  cursor: { line: 1, col: 1 },
  currentLineInfo: null,
  currentLineHints: [],
  formGlyph: "off",

  setCursor(line, col) {
    set({ cursor: { line, col } });
  },

  setLineInfo(info, hints) {
    set({ currentLineInfo: info, currentLineHints: hints });
  },

  setFormGlyph(mode) {
    set({ formGlyph: mode });
    get()._documents?.setFormGlyph(mode);
  },
});
