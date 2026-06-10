/**
 * Output slice — the append-only log behind the Output tool window
 * (docs/studio-shell-spec.md §4: compile timings, wasm/runtime errors that
 * aren't source diagnostics; previously this information was dropped).
 *
 * Producers: the compile callbacks (brink-studio/main.tsx) log compile
 * success/failure, and the session slice logs story runtime errors. The log
 * is capped at OUTPUT_LOG_LIMIT entries (unbounded-growth guard) — appending
 * past the cap drops the oldest entries.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";

/** Maximum retained Output entries — any accumulation needs a limit. */
export const OUTPUT_LOG_LIMIT = 500;

export type OutputSource = "compile" | "story";

export interface OutputEntry {
  /** Epoch milliseconds (Date.now()) when the entry was appended. */
  timestamp: number;
  source: OutputSource;
  message: string;
}

export interface OutputSlice {
  /** Append-only log, oldest first; capped at OUTPUT_LOG_LIMIT. */
  outputEntries: OutputEntry[];

  appendOutput(source: OutputSource, message: string): void;
  clearOutput(): void;
}

export const createOutputSlice: StateCreator<StudioState, [], [], OutputSlice> = (set) => ({
  outputEntries: [],

  appendOutput(source, message) {
    const entry: OutputEntry = { timestamp: Date.now(), source, message };
    set((state) => ({
      outputEntries: [...state.outputEntries, entry].slice(-OUTPUT_LOG_LIMIT),
    }));
  },

  clearOutput() {
    set({ outputEntries: [] });
  },
});
