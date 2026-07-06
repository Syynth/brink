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
import { programChecksum } from "@brink-lang/web";
import { sortDiagnostics } from "@brink-lang/editor";

// Canonical Problems ordering — the published boundary helper (#369):
// file path, then start offset, then errors before warnings. Re-exported
// here for existing @brink/studio-store consumers.
export { sortDiagnostics };

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
   * Source-identity checksum of the latest successful compile's bytes
   * (`"0x{:08x}"`, matches `ProgramModel.checksum`). The studio compares this
   * to the *running* program's `programChecksum` to detect degraded mode
   * (live-inspector spec §5, #181). `null` when the latest compile failed.
   */
  compiledChecksum: string | null;
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
  compiledChecksum: null,
  storyGraph: null,
  externalCheck: "error",

  setCompileResult(outline, diagnostics, diagnosticsList, storyBytes) {
    // Identity of this compile, for the degraded-mode comparison (spec §5).
    // A failed compile (null bytes) clears it; a decode failure leaves it null
    // rather than throwing into the compile path.
    let compiledChecksum: string | null = null;
    if (storyBytes) {
      try {
        compiledChecksum = programChecksum(storyBytes);
      } catch {
        compiledChecksum = null;
      }
    }
    set({
      outline,
      diagnostics,
      diagnosticsList: sortDiagnostics(diagnosticsList),
      storyBytes,
      compiledChecksum,
    });
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
