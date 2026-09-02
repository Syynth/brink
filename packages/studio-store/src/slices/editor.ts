/**
 * Editor slice — cursor position and current line metadata.
 *
 * Updated at high frequency by CM6 editor callbacks.
 */

import type { StateCreator } from "zustand";
import type { DialogueDialect, FormGlyphMode, LineInfo } from "@brink-lang/editor";
import type { StudioState } from "../index.js";
import { clampAppFontSize, clampEditorFontSize } from "@brink-lang/editor";
import type { KeyHint } from "../types.js";

export interface EditorSlice {
  cursor: { line: number; col: number };
  currentLineInfo: LineInfo | null;
  currentLineHints: KeyHint[];
  /** The project's resolved dialogue dialect (#3387/#3389, RULED
   *  2026-08-30) — `brink.toml [dialogue]` after preset merge, or `null`
   *  when the project declares none. Mirrored here from the session at
   *  every config apply so the Player folds delivered lines into runs
   *  with the SAME artifact the editor classifies with. */
  projectDialect: DialogueDialect | null;
  setProjectDialect(dialect: DialogueDialect | null): void;
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
  /** Inlay hints (`: string`-style type/name annotations) — Settings.
   *  Applied live to all editors, and to editors opened after (#3350).
   *  Default ON (current behavior). */
  showInlayHints: boolean;
  /** Editor text size in px (beta feedback 2026-08-25). Mirrored onto the
   *  studio root as `--bs-editor-font-size`, which the CM6 theme reads. */
  editorFontSize: number;
  /** App-wide UI text size in px — mirrored onto the root as
   *  `--bs-font-base`, which the whole type scale derives from. */
  appFontSize: number;

  setCursor(line: number, col: number): void;
  setLineInfo(info: LineInfo | null, hints: KeyHint[]): void;
  setFormGlyph(mode: FormGlyphMode): void;
  setAutoOpenForm(on: boolean): void;
  setShowGutters(on: boolean): void;
  setShowInlayHints(on: boolean): void;
  /** Set an absolute size (clamped to the usable range). */
  setEditorFontSize(px: number): void;
  /** Step the size by `delta` px (clamped) — the zoom in/out commands. */
  adjustEditorFontSize(delta: number): void;
  setAppFontSize(px: number): void;
}

export const createEditorSlice: StateCreator<StudioState, [], [], EditorSlice> = (set, get) => ({
  cursor: { line: 1, col: 1 },
  currentLineInfo: null,
  currentLineHints: [],
  projectDialect: null,
  formGlyph: "off",
  autoOpenForm: false,
  showGutters: true,
  showInlayHints: true,
  editorFontSize: 14,
  appFontSize: 12,

  setCursor(line, col) {
    set({ cursor: { line, col } });
  },

  setLineInfo(info, hints) {
    set({ currentLineInfo: info, currentLineHints: hints });
  },

  setProjectDialect(dialect) {
    set({ projectDialect: dialect });
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

  setShowInlayHints(on) {
    set({ showInlayHints: on });
    get()._documents?.setInlayHints(on);
  },

  setEditorFontSize(px) {
    set({ editorFontSize: clampEditorFontSize(px) });
  },

  adjustEditorFontSize(delta) {
    set({ editorFontSize: clampEditorFontSize(get().editorFontSize + delta) });
  },

  setAppFontSize(px) {
    set({ appFontSize: clampAppFontSize(px) });
  },
});
