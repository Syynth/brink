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
 * The durable half — which severities are shown, and whether rows are
 * grouped by file — round-trips through localStorage (ruled 2026-08-25:
 * "on by default, and persisted across refreshes, same with the toggles").
 * These are how an author reads their problem list; re-picking them every
 * launch is exactly the kind of small tax that makes a panel annoying.
 *
 * The filter TEXT deliberately does NOT persist. A query restored into a
 * closed filter row is a panel silently hiding rows with no visible cause —
 * the same failure the clear-on-close rule below exists to prevent.
 *
 * Info stays ON by default: E189 TODO notes are Info, and defaulting them
 * off would silently hide diagnostics that are visible today.
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
  /** Apply persisted preferences at boot (mount.tsx). */
  applyProblemsPrefs(prefs: ProblemsPrefs): void;
  /** Injected persistence sink; null until the app binds it. Keeps the
   *  slice free of a direct `window` dependency, like `_notify`. */
  _persistProblemsPrefs: ((prefs: ProblemsPrefs) => void) | null;
  setProblemsPrefsSink(sink: (prefs: ProblemsPrefs) => void): void;
}

/** The persisted subset — the view preferences, never the filter text. */
export interface ProblemsPrefs {
  severities: Record<ProblemSeverityBucket, boolean>;
  grouped: boolean;
}

export const PROBLEMS_STORAGE_KEY = "brink-studio.problems.v1";

const DEFAULT_PREFS: ProblemsPrefs = {
  severities: { error: true, warning: true, info: true },
  grouped: true,
};

/** Load persisted preferences. Never throws; defaults on anything odd. */
export function loadProblemsPrefs(storage: Pick<Storage, "getItem">): ProblemsPrefs {
  let raw: string | null;
  try {
    raw = storage.getItem(PROBLEMS_STORAGE_KEY);
  } catch {
    return DEFAULT_PREFS;
  }
  if (raw === null || raw === "") return DEFAULT_PREFS;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_PREFS;
  }
  const obj = parsed as { severities?: unknown; grouped?: unknown } | null;
  const sev = (obj?.severities ?? {}) as Record<string, unknown>;
  return {
    // Only an explicit `false` hides a severity: a partial or older record
    // must never silently hide diagnostics.
    severities: {
      error: sev.error !== false,
      warning: sev.warning !== false,
      info: sev.info !== false,
    },
    grouped: obj?.grouped !== false,
  };
}

/** Persist preferences. Storage failures degrade to in-session. */
export function saveProblemsPrefs(
  storage: Pick<Storage, "setItem">,
  prefs: ProblemsPrefs,
): void {
  try {
    storage.setItem(PROBLEMS_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Quota/denied — the choice still applies for this session.
  }
}

export const createProblemsSlice: StateCreator<StudioState, [], [], ProblemsSlice> = (
  set,
  get,
) => ({
  problemsSeverities: { error: true, warning: true, info: true },
  problemsFilter: "",
  problemsFilterOpen: false,
  // Grouped by default (ruled): a flat list of every diagnostic in a
  // project reads as noise; per-file sections are how you actually scan it.
  problemsGrouped: true,
  problemsCollapsedFiles: {},

  _persistProblemsPrefs: null,

  setProblemsPrefsSink(sink) {
    set({ _persistProblemsPrefs: sink });
  },

  applyProblemsPrefs(prefs) {
    set({ problemsSeverities: prefs.severities, problemsGrouped: prefs.grouped });
  },

  toggleProblemSeverity(bucket) {
    const current = get().problemsSeverities;
    const severities = { ...current, [bucket]: !current[bucket] };
    set({ problemsSeverities: severities });
    get()._persistProblemsPrefs?.({ severities, grouped: get().problemsGrouped });
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
    const grouped = !get().problemsGrouped;
    set({ problemsGrouped: grouped });
    get()._persistProblemsPrefs?.({ severities: get().problemsSeverities, grouped });
  },

  toggleProblemsFileCollapsed(file) {
    const current = get().problemsCollapsedFiles;
    set({ problemsCollapsedFiles: { ...current, [file]: !current[file] } });
  },
});
