/**
 * Editor slice — cursor position and current line metadata.
 *
 * Updated at high frequency by CM6 editor callbacks.
 */

import type { StateCreator } from "zustand";
import type { FormGlyphMode, LineInfo } from "@brink-lang/editor";
import type { StudioState } from "../index.js";
import type { KeyHint } from "../types.js";

export interface EditorSlice {
  cursor: { line: number; col: number };
  currentLineInfo: LineInfo | null;
  currentLineHints: KeyHint[];
  /** Inline argument-form glyph mode (Settings; applied live to all editors). */
  formGlyph: FormGlyphMode;
  /** Auto-open the Form on accepting a function completion (Settings). */
  autoOpenForm: boolean;
  /** Editor gutters (line numbers, structure rails, fold/play markers) —
   *  Settings + editor context menu. Rendering-only: `App` mirrors it as a
   *  root class the stylesheet acts on; no CM reconfiguration. Besides the
   *  visual preference, hiding gutters removes a WebKit layout cost that
   *  scales with visible gutter elements (#3119), so it doubles as the
   *  interim latency escape hatch on large projects in the desktop app. */
  showGutters: boolean;

  setCursor(line: number, col: number): void;
  setLineInfo(info: LineInfo | null, hints: KeyHint[]): void;
  setFormGlyph(mode: FormGlyphMode): void;
  setAutoOpenForm(on: boolean): void;
  setShowGutters(on: boolean): void;
}

export const createEditorSlice: StateCreator<StudioState, [], [], EditorSlice> = (set, get) => ({
  cursor: { line: 1, col: 1 },
  currentLineInfo: null,
  currentLineHints: [],
  formGlyph: "off",
  autoOpenForm: false,
  showGutters: true,

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

  setAutoOpenForm(on) {
    set({ autoOpenForm: on });
    get()._documents?.setAutoOpen(on);
  },

  setShowGutters(on) {
    set({ showGutters: on });
  },
});
