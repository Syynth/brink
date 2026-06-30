/**
 * Conflict slice — external-conflict state for the merge view (issue #320,
 * Track V).
 *
 * The B1 hook ({@link ProjectSession}'s `onFileConflict`) fires when the host
 * rewrites a file the studio has an unsaved, divergent buffer for: the buffer
 * is kept (the safe default) and the path flagged conflicted. This slice
 * mirrors those conflicts so the merge surface (the banner + 2-way MergeView)
 * can render them, and routes the three resolutions back through the project's
 * FileChangeHub baseline/dirty seam:
 *
 *  - Use disk   → `ProjectSession.resolveConflictUseDisk(path, disk)`
 *  - Keep mine  → `ProjectSession.resolveConflictKeepMine(path)`
 *  - Merged     → `ProjectSession.resolveConflictMerged(path, merged)`
 *
 * Each resolution also drops the conflict from this slice so the surface tears
 * down. Conflicts are keyed by path (one live conflict per file); a re-fired
 * conflict for the same path replaces the prior entry.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { FileConflict } from "../types.js";

export interface ConflictSlice {
  /**
   * Active external conflicts by path (one per file). Sorted reads come from
   * {@link conflictPaths}; the map preserves identity for the merge view.
   */
  conflicts: Record<string, FileConflict>;

  /** Record (or replace) a conflict for its path — called by the mount.tsx
   *  `onFileConflict` bridge when the B1 hook fires. */
  setConflict(conflict: FileConflict): void;
  /** Drop a conflict for `path` without resolving it (e.g. the doc was
   *  closed); the surface tears down. The path's dirty/baseline state is
   *  untouched — use the resolve actions to actually reconcile. */
  clearConflict(path: string): void;

  /** Resolve `path` by taking the on-disk text (discard the studio buffer). */
  resolveUseDisk(path: string): void;
  /** Resolve `path` by keeping the studio buffer (stays dirty). */
  resolveKeepMine(path: string): void;
  /** Resolve `path` with a hand-merged result. */
  resolveMerge(path: string, merged: string): void;
}

export const createConflictSlice: StateCreator<StudioState, [], [], ConflictSlice> = (
  set,
  get,
) => {
  const drop = (path: string): void => {
    set((s) => {
      if (!(path in s.conflicts)) return {};
      const next = { ...s.conflicts };
      delete next[path];
      return { conflicts: next };
    });
  };

  return {
    conflicts: {},

    setConflict(conflict) {
      set((s) => ({ conflicts: { ...s.conflicts, [conflict.path]: conflict } }));
    },

    clearConflict(path) {
      drop(path);
    },

    resolveUseDisk(path) {
      const conflict = get().conflicts[path];
      if (!conflict) return;
      get()._project?.resolveConflictUseDisk(path, conflict.disk);
      drop(path);
    },

    resolveKeepMine(path) {
      if (!get().conflicts[path]) return;
      get()._project?.resolveConflictKeepMine(path);
      drop(path);
    },

    resolveMerge(path, merged) {
      if (!get().conflicts[path]) return;
      get()._project?.resolveConflictMerged(path, merged);
      drop(path);
    },
  };
};

/** Sorted paths with a live conflict (deterministic; for badging). */
export function conflictPaths(conflicts: Record<string, FileConflict>): string[] {
  return Object.keys(conflicts).sort();
}
