/**
 * Compile slice — outline, diagnostics, and compiled story bytes.
 *
 * Updated on debounced compile cycles. Alongside the summary counts (status
 * bar, strip badge), the full structured diagnostic list is stored for the
 * Problems tool window (docs/studio-shell-spec.md §4) — rows resolve to
 * source Locations and dispatch `editor.reveal` (§6.1).
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { Diagnostic, FileOutline, StoryGraph } from "@brink/wasm-types";

/**
 * Canonical Problems ordering (deterministic): file path, then start offset,
 * then errors before warnings, then end offset and message as tiebreakers.
 */
export function sortDiagnostics(diagnostics: readonly Diagnostic[]): Diagnostic[] {
  return [...diagnostics].sort((a, b) => {
    if (a.file !== b.file) return a.file < b.file ? -1 : 1;
    if (a.start !== b.start) return a.start - b.start;
    if (a.severity !== b.severity) return a.severity === "Error" ? -1 : 1;
    if (a.end !== b.end) return a.end - b.end;
    if (a.message !== b.message) return a.message < b.message ? -1 : 1;
    return 0;
  });
}

/**
 * The one real diagnostic severity knob (Settings document, #93): whether
 * manifest-driven external-function checks report as errors or are off.
 */
export type ExternalCheckLevel = "error" | "off";

export interface CompileSlice {
  outline: FileOutline[];
  diagnostics: { errors: number; warnings: number };
  /** Full diagnostic list from the latest compile, in canonical order. */
  diagnosticsList: Diagnostic[];
  storyBytes: Uint8Array | null;
  /**
   * Whole-project story graph for the Story Graph document (#97, spec §4.1).
   * Refreshed on each successful compile; a failed compile keeps the last
   * good graph (compile-bound, like `programInkt`). `null` until the first
   * successful compile.
   */
  storyGraph: StoryGraph | null;
  /** External-function checking severity (mirrors the wasm session). */
  externalCheck: ExternalCheckLevel;

  setCompileResult(
    outline: FileOutline[],
    diagnostics: { errors: number; warnings: number },
    diagnosticsList: Diagnostic[],
    storyBytes: Uint8Array | null,
  ): void;
  /** Replace the story graph (called on each successful compile). */
  setStoryGraph(graph: StoryGraph): void;
  compile(): void;
  convertLineToType(sigil: string): void;
  /**
   * Set external-function checking severity. Applies to the wasm session
   * and recompiles when a project is bound; called before `initialize`
   * (bootstrap restore) it only seeds the state — `initialize` applies it
   * to the session before the first compile.
   */
  setExternalCheck(level: ExternalCheckLevel): void;
}

export const createCompileSlice: StateCreator<StudioState, [], [], CompileSlice> = (set, get) => ({
  outline: [],
  diagnostics: { errors: 0, warnings: 0 },
  diagnosticsList: [],
  storyBytes: null,
  storyGraph: null,
  externalCheck: "error",

  setCompileResult(outline, diagnostics, diagnosticsList, storyBytes) {
    set({ outline, diagnostics, diagnosticsList: sortDiagnostics(diagnosticsList), storyBytes });
  },

  setStoryGraph(graph) {
    set({ storyGraph: graph });
  },

  compile() {
    get()._documents?.triggerCompile();
  },

  convertLineToType(sigil) {
    get()._documents?.convertLineToType(sigil);
  },

  setExternalCheck(level) {
    if (get().externalCheck === level) return;
    set({ externalCheck: level });
    const project = get()._project;
    if (project !== null) {
      project.getSession().setExternalCheck(level);
      get()._documents?.triggerCompile();
    }
  },
});
