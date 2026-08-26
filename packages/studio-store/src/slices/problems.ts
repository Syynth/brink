/**
 * Problems panel view state (beta feedback 2026-08-25: "sorted by file,
 * filtered, and toggles for individual severity levels").
 *
 * Why the store and not React state: the controls live in the tool
 * window's CHROME HEADER (`ToolWindowDescriptor.actions`), which the shell
 * renders in a different subtree from the panel body. The two can only
 * share state through a store — the same reason the strip badge reads the
 * store rather than taking a prop.
 *
 * Transient by design, like the search slice: nothing here persists across
 * restarts (that's the separate layout-persistence question).
 *
 * DEFAULTS PRESERVE TODAY'S BEHAVIOR — every severity visible, ungrouped,
 * filter closed — so adding the controls changes nothing until the user
 * touches one. In particular Info stays ON: E189 TODO notes are Info, and
 * defaulting them off would silently hide diagnostics that are visible
 * today.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";

/** The three buckets the panel's toggles expose. Info and Hint share one:
 *  both are advisory, and the rows already render them identically. */
export type ProblemSeverityBucket = "error" | "warning" | "info";

export interface ProblemsSlice {
  /** Which severity buckets are shown. */
  problemsSeverities: Readonly<Record<ProblemSeverityBucket, boolean>>;
  /** Case-insensitive filter over message + location; "" = no filter. */
  problemsFilter: string;
  /** Whether the filter row is revealed (the funnel button's state). */
  problemsFilterOpen: boolean;
  /** Group rows into collapsible per-file sections. */
  problemsGrouped: boolean;
  /** Collapsed file sections while grouped, keyed by path. */
  problemsCollapsedFiles: Readonly<Record<string, boolean>>;

  toggleProblemSeverity(bucket: ProblemSeverityBucket): void;
  setProblemsFilter(query: string): void;
  /** Toggle the filter row. Closing it also clears the query, so a hidden
   *  filter can never silently hide rows. */
  toggleProblemsFilter(): void;
  toggleProblemsGrouped(): void;
  toggleProblemsFileCollapsed(file: string): void;
}

export const createProblemsSlice: StateCreator<StudioState, [], [], ProblemsSlice> = (
  set,
  get,
) => ({
  problemsSeverities: { error: true, warning: true, info: true },
  problemsFilter: "",
  problemsFilterOpen: false,
  problemsGrouped: false,
  problemsCollapsedFiles: {},

  toggleProblemSeverity(bucket) {
    const current = get().problemsSeverities;
    set({ problemsSeverities: { ...current, [bucket]: !current[bucket] } });
  },

  setProblemsFilter(query) {
    set({ problemsFilter: query });
  },

  toggleProblemsFilter() {
    const open = !get().problemsFilterOpen;
    // Clearing on close is the load-bearing half: a filter you can't see is
    // a filter you can't explain, and "where did my errors go" is the bug
    // that pattern always produces.
    set({ problemsFilterOpen: open, problemsFilter: open ? get().problemsFilter : "" });
  },

  toggleProblemsGrouped() {
    set({ problemsGrouped: !get().problemsGrouped });
  },

  toggleProblemsFileCollapsed(file) {
    const current = get().problemsCollapsedFiles;
    set({ problemsCollapsedFiles: { ...current, [file]: !current[file] } });
  },
});
