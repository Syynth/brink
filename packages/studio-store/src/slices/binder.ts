/**
 * Binder slice — collapsed state, multi-select, and undo stack for the
 * file/symbol tree.
 *
 * Structural operations announce themselves through the injected notifier
 * (`_notify`, see StoreNotification in ../index.ts): the post-move
 * notification carries an Undo action that dispatches the `binder.undo`
 * command (registered at the app boundary, gated on a non-empty undo stack) —
 * command-only actions per spec §7.5, and the store stays shell-free.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { MoveResult } from "@brink/wasm-types";

// ── Undo entry ──────────────────────────────────────────────────────

/**
 * An undoable binder operation. Two shapes:
 *
 * - `edits` — content was rewritten in place (structural moves, search
 *   replace). Undo restores each snapshot through `applyEdit` (egresses as
 *   `modified`, the correct type for a file the host still has).
 * - `recreate` — files were deleted. Undo must *re-create* them, not rewrite
 *   them: the host no longer has these paths, so restoring egresses as
 *   `created` (via `project.addFile`) and the file tab is reopened.
 * - `rename` — a file was renamed/moved. Undo is the inverse rename
 *   (`from`→`to`); the rename op is self-inverting (INCLUDE rewrites included),
 *   so no snapshot is needed.
 */
export type UndoEntry =
  | {
      kind: "edits";
      description: string;
      snapshots: Array<{ path: string; source: string }>;
    }
  | {
      kind: "recreate";
      description: string;
      files: Array<{ path: string; source: string }>;
    }
  | {
      kind: "rename";
      description: string;
      /** Undo applies each inverse rename (`from`→`to`), in reverse order. A
       *  single file rename has one entry; a folder rename batches all its
       *  files into one undoable step. */
      renames: Array<{ from: string; to: string }>;
    };

// ── Slice interface ─────────────────────────────────────────────────

export interface BinderSlice {
  collapsed: Set<string>;
  selectedKeys: Set<string>;
  focusedKey: string | null;
  undoStack: UndoEntry[];

  toggleCollapsed(key: string): void;
  selectKey(key: string, multi: boolean): void;
  clearSelection(): void;
  setFocusedKey(key: string | null): void;
  applyMoveResult(
    result: MoveResult,
    description: string,
    affectedPaths: string[],
  ): Promise<void>;
  deleteFile(path: string): Promise<void>;
  deleteFolder(prefix: string, paths: string[]): Promise<void>;
  renameFile(oldPath: string, newPath: string): Promise<void>;
  moveFile(oldPath: string, newPath: string): Promise<void>;
  renameFolder(oldPrefix: string, newPrefix: string, paths: string[]): Promise<void>;
  undo(): Promise<void>;
}

// ── Helpers ─────────────────────────────────────────────────────────

/** Parse a binder key into its parts. Returns kind + parentKey. */
function parseKey(key: string): { kind: "file" | "knot" | "stitch"; parentKey: string | null } {
  const parts = key.split("::");
  if (parts.length === 3) return { kind: "stitch", parentKey: `${parts[0]}::${parts[1]}` };
  if (parts.length === 2) return { kind: "knot", parentKey: parts[0]! };
  return { kind: "file", parentKey: null };
}

/** Check if two keys are same-kind siblings. */
function areSameKindSiblings(a: string, b: string): boolean {
  const pa = parseKey(a);
  const pb = parseKey(b);
  return pa.kind === pb.kind && pa.parentKey === pb.parentKey;
}

// ── Slice creator ───────────────────────────────────────────────────

export const createBinderSlice: StateCreator<StudioState, [], [], BinderSlice> = (set, get) => ({
  collapsed: new Set<string>(),
  selectedKeys: new Set<string>(),
  focusedKey: null,
  undoStack: [],

  toggleCollapsed(key) {
    const next = new Set(get().collapsed);
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
    }
    set({ collapsed: next });
  },

  selectKey(key, multi) {
    if (!multi) {
      set({ selectedKeys: new Set([key]), focusedKey: key });
      return;
    }
    const current = get().selectedKeys;
    // Validate same-kind sibling constraint
    if (current.size > 0) {
      const existing = current.values().next().value!;
      if (!areSameKindSiblings(existing, key)) {
        // Invalid multi-select: replace with just this key
        set({ selectedKeys: new Set([key]), focusedKey: key });
        return;
      }
    }
    const next = new Set(current);
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
    }
    set({ selectedKeys: next, focusedKey: key });
  },

  clearSelection() {
    set({ selectedKeys: new Set(), focusedKey: null });
  },

  setFocusedKey(key) {
    set({ focusedKey: key });
  },

  async applyMoveResult(result, description, affectedPaths) {
    const state = get();
    const project = state._project;
    const documents = state._documents;
    if (!project || !documents) return;

    const session = project.getSession();

    // Every file this move touches: the primary file plus any cross-file
    // reference edits (e.g. diverts in other files that point at a moved
    // symbol). De-duplicated, preserving order.
    const touchedPaths = [
      ...new Set([...affectedPaths, ...result.cross_file_edits.map((e) => e.path)]),
    ];

    // 1. Snapshot current sources for undo (all touched files, so undo can
    //    restore the cross-file edits too).
    const snapshots: Array<{ path: string; source: string }> = [];
    for (const path of touchedPaths) {
      const source = session.getFileSource(path);
      if (source != null) {
        snapshots.push({ path, source });
      }
    }

    // 2. Apply new_source to the target file (from result.path) — through
    //    the project's shared apply-edits seam (#137), so the provider
    //    write-back and the host egress callback see structural ops too.
    if (result.new_source != null && result.path) {
      project.applyEdit(result.path, result.new_source);
    }

    // 3. Apply cross-file reference edits — each carries the full new source
    //    of an affected file, keyed by path.
    for (const edit of result.cross_file_edits) {
      project.applyEdit(edit.path, edit.new_source);
    }

    // 4. Push undo entry
    const undoStack: UndoEntry[] = [
      ...state.undoStack,
      { kind: "edits", description, snapshots },
    ];

    // 5. Refresh editor views for affected files: mounted views reload their
    //    content (symbol views re-resolve their range), cached states rebuild
    //    on next mount.
    for (const path of touchedPaths) {
      documents.invalidateFile(path);
    }

    // 6. Trigger recompile (refreshes outline)
    documents.triggerCompile();

    // 7. Notify, with Undo dispatching the binder.undo command (spec §7.5).
    set({ undoStack });
    get()._notify?.({
      severity: "info",
      source: "binder",
      message: description,
      actions: [{ label: "Undo", commandId: "binder.undo" }],
    });
  },

  async deleteFile(path) {
    await deleteFilesWithUndo(get, set, [path], `Deleted ${path}`);
  },

  async deleteFolder(prefix, paths) {
    if (paths.length === 0) return;
    const label = prefix.replace(/\/$/, "");
    await deleteFilesWithUndo(
      get,
      set,
      paths,
      `Deleted ${label}/ (${paths.length} file${paths.length === 1 ? "" : "s"})`,
    );
  },

  async renameFile(oldPath, newPath) {
    await renameWithUndo(get, set, oldPath, newPath, "Renamed");
  },

  async moveFile(oldPath, newPath) {
    await renameWithUndo(get, set, oldPath, newPath, "Moved");
  },

  async renameFolder(oldPrefix, newPrefix, paths) {
    if (oldPrefix === newPrefix || paths.length === 0) return;
    const renames: Array<{ from: string; to: string }> = [];
    for (const old of paths) {
      const moved = newPrefix + old.slice(oldPrefix.length);
      if (await applyRename(get, old, moved)) {
        renames.unshift({ from: moved, to: old }); // reverse order for undo
      }
    }
    if (renames.length === 0) return;
    const label = `Renamed ${oldPrefix.replace(/\/$/, "")}/ → ${newPrefix.replace(/\/$/, "")}/`;
    set({ undoStack: [...get().undoStack, { kind: "rename", description: label, renames }] });
    get()._notify?.({
      severity: "info",
      source: "binder",
      message: label,
      actions: [{ label: "Undo", commandId: "binder.undo" }],
    });
  },

  async undo() {
    const state = get();
    const project = state._project;
    const documents = state._documents;
    if (!project || !documents) return;

    const stack = [...state.undoStack];
    const entry = stack.pop();
    if (!entry) return;

    if (entry.kind === "edits") {
      // Restore each snapshot — through the shared apply-edits seam (#137):
      // an undo changes file content like any other edit, and the host must
      // see the reverted text.
      for (const { path, source } of entry.snapshots) {
        project.applyEdit(path, source);
      }
      // Refresh editor views for the restored files.
      for (const { path } of entry.snapshots) {
        documents.invalidateFile(path);
      }
    } else if (entry.kind === "recreate") {
      // Re-create deleted files: the host has no such paths, so this egresses
      // as `created` (via addFile), then reopen each file's tab.
      for (const { path, source } of entry.files) {
        await project.addFile(path, source);
      }
      for (const { path } of entry.files) {
        get().openTarget({ kind: "file", path }, true);
      }
    } else {
      // Inverse rename(s) — the op is self-inverting (INCLUDE rewrites
      // included). Reverse order so a folder batch unwinds cleanly.
      for (const { from, to } of [...entry.renames].reverse()) {
        await applyRename(get, from, to);
      }
    }

    // Trigger recompile
    documents.triggerCompile();

    set({ undoStack: stack });
    get()._notify?.({
      severity: "info",
      source: "binder",
      message: `Undid: ${entry.description}`,
    });
  },
});

// ── Delete helper ───────────────────────────────────────────────────

/**
 * Snapshot → close tabs → delete → recompile, pushing a single `recreate`
 * undo entry for the whole batch. Teardown order (the destructive risk):
 * snapshot first so undo can fully reconstruct; close tabs before the file
 * leaves the session so no mounted view reads a dead path; delete (provider +
 * session + `deleted` egress) last.
 */
async function deleteFilesWithUndo(
  get: () => StudioState,
  set: (partial: Partial<StudioState>) => void,
  paths: string[],
  description: string,
): Promise<void> {
  const state = get();
  const project = state._project;
  const documents = state._documents;
  if (!project || !documents) return;

  const session = project.getSession();

  // 1. Snapshot content for undo (skip files that have already vanished).
  const files: Array<{ path: string; source: string }> = [];
  for (const path of paths) {
    const source = session.getFileSource(path);
    if (source != null) files.push({ path, source });
  }
  if (files.length === 0) return;

  // 2. Close every open view for each file, then 3. delete it.
  for (const { path } of files) {
    state.closeDocsForPath(path);
    await project.deleteFile(path);
  }

  // 4. Recompile (refreshes outline + surfaces any now-dangling INCLUDEs).
  documents.triggerCompile();

  // 5. Push undo + notify with Undo (binder.undo command, spec §7.5).
  //    Read the stack fresh (deletes are async — don't clobber concurrent ops).
  set({ undoStack: [...get().undoStack, { kind: "recreate", description, files }] });
  get()._notify?.({
    severity: "info",
    source: "binder",
    message: description,
    actions: [{ label: "Undo", commandId: "binder.undo" }],
  });
}

// ── Rename / move helper ────────────────────────────────────────────

/**
 * Apply a rename/move: close the old file's tabs before its key leaves the
 * session, rename it (rewriting INCLUDE references via the session op), reopen
 * it at the new path, refresh referrer views, and recompile. Returns false
 * (with an error notification) on failure. Pushes no undo entry — callers
 * manage the stack.
 */
async function applyRename(
  get: () => StudioState,
  oldPath: string,
  newPath: string,
): Promise<boolean> {
  const state = get();
  const project = state._project;
  const documents = state._documents;
  if (!project || !documents) return false;

  state.closeDocsForPath(oldPath);

  let referrers: string[];
  try {
    referrers = await project.renameFile(oldPath, newPath);
  } catch (e) {
    get()._notify?.({
      severity: "error",
      source: "binder",
      message: e instanceof Error ? e.message : `cannot rename ${oldPath}`,
    });
    return false;
  }

  get().openTarget({ kind: "file", path: newPath }, true);
  for (const path of referrers) {
    documents.invalidateFile(path);
  }
  documents.triggerCompile();
  return true;
}

/**
 * Rename/move with an undo entry. Undo is the inverse rename — no snapshot
 * needed, since the op round-trips (INCLUDE rewrites included).
 */
async function renameWithUndo(
  get: () => StudioState,
  set: (partial: Partial<StudioState>) => void,
  oldPath: string,
  newPath: string,
  verb: string,
): Promise<void> {
  if (oldPath === newPath) return;
  if (!(await applyRename(get, oldPath, newPath))) return;

  const description = `${verb} ${oldPath} → ${newPath}`;
  set({
    undoStack: [
      ...get().undoStack,
      { kind: "rename", description, renames: [{ from: newPath, to: oldPath }] },
    ],
  });
  get()._notify?.({
    severity: "info",
    source: "binder",
    message: description,
    actions: [{ label: "Undo", commandId: "binder.undo" }],
  });
}
