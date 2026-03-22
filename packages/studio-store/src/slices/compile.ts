/**
 * Compile slice — outline, diagnostics, and compiled story bytes.
 *
 * Updated on debounced compile cycles.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { FileOutline } from "@brink/wasm-types";

export interface CompileSlice {
  outline: FileOutline[];
  diagnostics: { errors: number; warnings: number };
  storyBytes: Uint8Array | null;

  setCompileResult(
    outline: FileOutline[],
    diagnostics: { errors: number; warnings: number },
    storyBytes: Uint8Array | null,
  ): void;
  compile(): void;
  convertLineToType(sigil: string): void;
}

export const createCompileSlice: StateCreator<StudioState, [], [], CompileSlice> = (set, get) => ({
  outline: [],
  diagnostics: { errors: 0, warnings: 0 },
  storyBytes: null,

  setCompileResult(outline, diagnostics, storyBytes) {
    set({ outline, diagnostics, storyBytes });
  },

  compile() {
    get()._editorRef?.triggerCompile();
  },

  convertLineToType(sigil) {
    const editor = get()._editorRef;
    if (!editor) return;
    editor.convertLineToType(sigil);
  },
});
