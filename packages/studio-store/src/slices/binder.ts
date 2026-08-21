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
import type { StructuralResult, RenameDiagnostic } from "@brink/wasm-types";

// ── Undo entry ──────────────────────────────────────────────────────

/**
 * An undoable binder operation. Four shapes:
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
 * - `rename-dir` — a folder was renamed/moved. Undo is the inverse `rename_dir`
 *   call (prefixes swapped) — see the variant's own doc below. Unlike the
 *   other three shapes, undo can itself be refused (all-or-nothing, #2587):
 *   `undo()` must leave this entry on the stack rather than popping it when
 *   that happens (see `undo()`'s `rename-dir` branch).
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
       *  single file rename has one entry; a batch move (`moveFiles`) groups
       *  several independent per-file renames into one undoable step. NOT
       *  used for a folder rename — see `rename-dir` below. */
      renames: Array<{ from: string; to: string }>;
    }
  | {
      kind: "rename-dir";
      description: string;
      /** Undo re-applies the atomic `rename_dir` op with the prefixes
       *  swapped (`newPrefix` → `oldPrefix`) — the inverse of a whole-folder
       *  move is itself a whole-folder move, so undo gets the SAME
       *  single-snapshot consistency guarantee (#314) the forward move does,
       *  rather than falling back to a per-file loop (issue #2587). */
      oldPrefix: string;
      newPrefix: string;
    };

// ── Slice interface ─────────────────────────────────────────────────

export interface BinderSlice {
  collapsed: Set<string>;
  selectedKeys: Set<string>;
  focusedKey: string | null;
  undoStack: UndoEntry[];
  /** Whether the Binder's "Library" section (mounted `std/` files, issue
   *  #2306/#2343) is expanded. Collapsed by default, per the ruling. */
  libraryExpanded: boolean;

  toggleCollapsed(key: string): void;
  toggleLibraryExpanded(): void;
  selectKey(key: string, multi: boolean): void;
  clearSelection(): void;
  setFocusedKey(key: string | null): void;
  applyMoveResult(
    result: StructuralResult,
    description: string,
    affectedPaths: string[],
  ): Promise<void>;
  deleteFile(path: string): Promise<void>;
  deleteFolder(prefix: string, paths: string[]): Promise<void>;
  renameFile(oldPath: string, newPath: string): Promise<void>;
  moveFile(oldPath: string, newPath: string): Promise<void>;
  moveFiles(paths: string[], destPrefix: string): Promise<void>;
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
  libraryExpanded: false,

  toggleCollapsed(key) {
    const next = new Set(get().collapsed);
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
    }
    set({ collapsed: next });
  },

  toggleLibraryExpanded() {
    set({ libraryExpanded: !get().libraryExpanded });
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

    // A refused op (`ok: false`) carries no `new_source` and no cross-file
    // edits, so applying it writes nothing — but it would still push an undo
    // entry and raise this function's confirming toast, turning a refusal into
    // a reported success (#2543). Refuse it at the seam so no caller can make
    // that claim. The user-facing error belongs to the caller, which knows
    // what was attempted (`applyComputedRename`, `performSymbolRename`);
    // per-op reporting for the remaining structural ops is #2544.
    if (!result.ok) return;

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
    //    `applyEdit` refuses a mounted stdlib path (issue #2306/#2343) —
    //    the Binder offers no structural-move affordance on the Library
    //    section, but a cross-file edit (below) or a stale target could
    //    still name one; track refusals instead of silently no-opping a
    //    move the caller's toast reports as having succeeded.
    let skipped = 0;
    if (result.new_source != null && result.path) {
      if (!project.applyEdit(result.path, result.new_source)) skipped += 1;
    }

    // 3. Apply cross-file reference edits — each carries the full new source
    //    of an affected file, keyed by path.
    for (const edit of result.cross_file_edits) {
      if (!project.applyEdit(edit.path, edit.new_source)) skipped += 1;
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
    const skippedSuffix =
      skipped > 0 ? ` (skipped ${skipped} read-only ${skipped === 1 ? "file" : "files"})` : "";
    get()._notify?.({
      severity: skipped > 0 ? "warning" : "info",
      source: "binder",
      message: `${description}${skippedSuffix}`,
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

  async moveFiles(paths, destPrefix) {
    if (paths.length === 0) return;
    // Batch several files to a destination folder ("" = project root) as one
    // undoable step. Files already at the destination are skipped; a per-file
    // collision (applyRename → null) is dropped without aborting the rest.
    //
    // NOT covered by #2918: `applyRename` now also returns each successful
    // move's `safe`/`introducedDiagnostics` (below), but this batch path
    // doesn't aggregate them into the one summary notification the way
    // `renameWithUndo`/`renameFolder` do for a single move — #2918 scoped its
    // fix to `renameFile`/`renameDir` by name; a batch move breaking a
    // reference still applies silently today. Flagged as a follow-up on that
    // issue rather than folded into this fix.
    const renames: Array<{ from: string; to: string }> = [];
    for (const old of paths) {
      const base = old.split("/").pop() ?? old;
      const moved = destPrefix + base;
      if (moved === old) continue;
      if (await applyRename(get, old, moved)) {
        renames.unshift({ from: moved, to: old }); // reverse order for undo
      }
    }
    if (renames.length === 0) return;
    const n = renames.length;
    const dest = destPrefix === "" ? "project root" : destPrefix.replace(/\/$/, "") + "/";
    const label = `Moved ${n} file${n === 1 ? "" : "s"} to ${dest}`;
    set({ undoStack: [...get().undoStack, { kind: "rename", description: label, renames }] });
    get()._notify?.({
      severity: "info",
      source: "binder",
      message: label,
      actions: [{ label: "Undo", commandId: "binder.undo" }],
    });
  },

  async renameFolder(oldPrefix, newPrefix, paths) {
    // `paths` is the Binder's own pre-flight list (from `outline`) — kept
    // only for this cheap early exit, matching the old per-file loop's quiet
    // no-op for an empty selection. It is NOT threaded into `renameDir`: the
    // atomic op discovers every file under `oldPrefix` itself, against its
    // own single pre-move snapshot (#314) — passing a client-computed list
    // would just be a second, possibly-stale source of truth for the same
    // question the op already answers authoritatively.
    if (oldPrefix === newPrefix || paths.length === 0) return;
    const result = await applyDirRename(get, oldPrefix, newPrefix);
    if (result === null || result.moved.length === 0) return;

    const label = `Renamed ${oldPrefix.replace(/\/$/, "")}/ → ${newPrefix.replace(/\/$/, "")}/`;
    set({
      undoStack: [
        ...get().undoStack,
        { kind: "rename-dir", description: label, oldPrefix, newPrefix },
      ],
    });
    notifyMoveResult(get, label, result);
  },

  async undo() {
    const state = get();
    const project = state._project;
    const documents = state._documents;
    if (!project || !documents) return;

    const stack = [...state.undoStack];
    const entry = stack.pop();
    if (!entry) return;

    let undoSkipped = 0;
    if (entry.kind === "edits") {
      // Restore each snapshot — through the shared apply-edits seam (#137):
      // an undo changes file content like any other edit, and the host must
      // see the reverted text. `applyEdit` refuses a mounted stdlib path
      // (issue #2306/#2343) — a snapshot can only name one if the original
      // edit somehow reached it (the structural-op path above already
      // tracks that), so this is defense-in-depth: track it rather than
      // let the undo notification claim a restore that didn't happen.
      for (const { path, source } of entry.snapshots) {
        if (!project.applyEdit(path, source)) undoSkipped += 1;
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
    } else if (entry.kind === "rename") {
      // Inverse rename(s) — the op is self-inverting (INCLUDE rewrites
      // included). Reverse order so a batch move unwinds cleanly.
      for (const { from, to } of [...entry.renames].reverse()) {
        await applyRename(get, from, to);
      }
    } else {
      // Inverse directory rename — same atomic op, prefixes swapped
      // (#2587): the forward move's single-snapshot consistency guarantee
      // applies to undo too, not just a per-file loop's worth of it.
      //
      // All-or-nothing means the inverse move can itself be refused (a
      // destination collision, or an empty folder) — `applyDirRename`
      // already raised the error notification in that case. Popping the
      // entry and then reporting "Undid: ..." below would claim a false
      // success on top of that error toast, and — worse — the entry would
      // be gone from the stack with nothing having actually moved, making
      // the refused undo unrecoverable through the undo stack. Return
      // before the stack is popped/persisted and before the "Undid:"
      // notification fires, leaving the entry exactly where it was
      // (issue #2916 review; mirrors `applyMoveResult`'s #2543 refuse-at-
      // the-seam rule for the forward direction).
      const result = await applyDirRename(get, entry.newPrefix, entry.oldPrefix);
      if (result === null) {
        return;
      }
    }

    // Trigger recompile
    documents.triggerCompile();

    set({ undoStack: stack });
    const undoSkippedSuffix =
      undoSkipped > 0
        ? ` (skipped ${undoSkipped} read-only ${undoSkipped === 1 ? "file" : "files"})`
        : "";
    get()._notify?.({
      severity: undoSkipped > 0 ? "warning" : "info",
      source: "binder",
      message: `Undid: ${entry.description}${undoSkippedSuffix}`,
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
 *
 * A mounted stdlib path (issue #2306/#2343) is filtered out before any of
 * that teardown runs — `project.deleteFile` refuses it and returns `false`,
 * but by then this function would already have closed the file's tabs via
 * `closeDocsForPath`, which is itself destructive to the user's view state.
 * Checking `project.isReadOnly` up front keeps a mounted path from ever
 * reaching snapshot/close/delete, and `deleteFile`'s boolean return is kept
 * as defense in depth for a path this filter didn't already catch.
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

  // 0. Filter out mounted stdlib paths — never snapshot, close, or delete them.
  let skipped = 0;
  const deletable: string[] = [];
  for (const path of paths) {
    if (project.isReadOnly(path)) {
      skipped += 1;
    } else {
      deletable.push(path);
    }
  }

  // 1. Snapshot content for undo (skip files that have already vanished).
  const files: Array<{ path: string; source: string }> = [];
  for (const path of deletable) {
    const source = session.getFileSource(path);
    if (source != null) files.push({ path, source });
  }
  if (files.length === 0) {
    if (skipped > 0) {
      get()._notify?.({
        severity: "warning",
        source: "binder",
        message: `Skipped ${skipped} read-only ${skipped === 1 ? "file" : "files"}: part of the read-only library`,
      });
    }
    return;
  }

  // 2. Close every open view for each file, then 3. delete it.
  for (const { path } of files) {
    state.closeDocsForPath(path);
    if (!(await project.deleteFile(path))) skipped += 1;
  }

  // 4. Recompile (refreshes outline + surfaces any now-dangling INCLUDEs).
  documents.triggerCompile();

  // 5. Push undo + notify with Undo (binder.undo command, spec §7.5).
  //    Read the stack fresh (deletes are async — don't clobber concurrent ops).
  set({ undoStack: [...get().undoStack, { kind: "recreate", description, files }] });
  const skippedSuffix =
    skipped > 0 ? ` (skipped ${skipped} read-only ${skipped === 1 ? "file" : "files"})` : "";
  get()._notify?.({
    severity: skipped > 0 ? "warning" : "info",
    source: "binder",
    message: `${description}${skippedSuffix}`,
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
 *
 * Off the paint path (#2776, generalizing #2767's remedy — spec §7.7.4):
 * `project.renameFile` runs the same op-agnostic breakage gate as
 * `moveStitch`/`promoteStitch`/`demoteKnot`, and defers its own heavy wasm
 * call internally via `scheduleIdleWork` (`ProjectSession.renameFile`,
 * `packages/ink-editor/src/project-session.ts`). This function is the
 * store-aware layer that call has none of, so the synchronous half of the
 * remedy lives here: commit `structuralOpPending` BEFORE the first `await`,
 * so React has something to paint before the deferred call can block the
 * main thread. Reuses the same status-bar affordance #2767 introduced
 * (`StructuralOpSegment`) rather than a notification (spec §7.5) — the
 * Binder has no per-row busy indicator to prefer instead. No staleness
 * re-check against a pre-idle snapshot: `renameFile` calls the wasm op fresh
 * against the session's live source when it actually runs and already
 * refuses cleanly (the catch below) if its target moved out from under it —
 * the same trust-the-op's-own-refusal reasoning `runGatedStructuralOp`
 * documents for the sibling ops.
 *
 * Clears via `clearStructuralOpPending(description)`, not
 * `setStructuralOpPending(null)` (issue #2794): this is a fire-and-forget
 * `void` dispatch, same as `runGatedStructuralOp`'s symbol-menu ops, and the
 * two share `structuralOpPending`. An overlapping symbol-menu op settling
 * after this one started (or vice versa) must not erase the other's
 * still-live indicator — compare-and-clear only removes the description
 * THIS call set.
 *
 * Returns `null` (with an error notification already raised) on a refused
 * rename, or the move's breakage-gate verdict (`safe`/`introducedDiagnostics`,
 * issue #2918) on success — `project.renameFile` computes this correctly but
 * used to have it discarded right here, the exact gap #2918 closes: a move
 * that broke a reference applied with nothing telling the caller. This
 * function still applies the move either way (the notification FLOOR, not a
 * preflight gate — see #2918/#324); it is the caller's job to decide how to
 * report an unsafe result, same as it already decides the undo entry and
 * description.
 */
async function applyRename(
  get: () => StudioState,
  oldPath: string,
  newPath: string,
): Promise<{ safe: boolean; introducedDiagnostics: RenameDiagnostic[] } | null> {
  const state = get();
  const project = state._project;
  const documents = state._documents;
  if (!project || !documents) return null;

  const pendingDescription = `Renaming ${oldPath} → ${newPath}`;
  state.setStructuralOpPending(pendingDescription);
  let result: { referrers: string[]; safe: boolean; introducedDiagnostics: RenameDiagnostic[] };
  try {
    result = await project.renameFile(oldPath, newPath);
  } catch (e) {
    // renameFile validates (and throws) before mutating the session, so a
    // failed rename — e.g. a name collision — must leave the open file and
    // its tabs untouched. Tear-down happens only on success, below.
    //
    // This is the pattern for reporting a refused structural operation
    // (`docs/studio-shell-spec.md` §7.5): an error-severity notification
    // carrying the op's own reason, tagged with the same `source` as the
    // operation's success toast. The knot/stitch rename's failure path
    // follows it — see `performSymbolRename` in studio-ui's
    // `symbolMenuActions.ts` (#2528).
    get()._notify?.({
      severity: "error",
      source: "binder",
      message: e instanceof Error ? e.message : `cannot rename ${oldPath}`,
    });
    return null;
  } finally {
    state.clearStructuralOpPending(pendingDescription);
  }

  // Re-key any open tabs/views for the file in place (preserve pin/split/
  // selection) rather than closing and reopening.
  state.renameDocPath(oldPath, newPath);
  for (const path of result.referrers) {
    documents.invalidateFile(path);
  }
  documents.triggerCompile();
  return { safe: result.safe, introducedDiagnostics: result.introducedDiagnostics };
}

/**
 * Apply a directory rename/move via the atomic `rename_dir` op (#314, wired
 * in #2587) — the directory analog of {@link applyRename}, used for both the
 * forward folder rename and its undo (the inverse of a whole-folder move is
 * itself a whole-folder move, just with the prefixes swapped, so both call
 * this one helper).
 *
 * Same off-paint-path shape as `applyRename`: `project.renameDir` runs the
 * same `gate_with_source` breakage gate as `renameFile`, deferred internally
 * via `deferGatedCall` (`ProjectSession.renameDir`'s own doc comment), so the
 * synchronous busy-state commit happens here, before the first `await`.
 *
 * All-or-nothing (#2587, decided in the PR that added this): the old
 * per-file loop silently skipped a colliding file and moved the rest, one
 * undo entry covering only the survivors. `rename_dir` refuses the WHOLE
 * move on any collision — no partial application — because a partial
 * directory move can only be computed by falling back to per-file INCLUDE
 * rewriting for the files that "succeed", which is exactly the
 * inconsistency #314 exists to prevent. A refusal here is reported as a
 * single error notification and nothing moves; the caller pushes no undo
 * entry.
 *
 * Returns the moved `{oldPath, newPath}` pairs plus the move's breakage-gate
 * verdict (`safe`/`introducedDiagnostics`, issue #2918 — same gap and same
 * fix shape as `applyRename` above: `project.renameDir` computed this
 * correctly all along but it used to be discarded right here) on success
 * (an empty `moved` list is a valid, non-error result — see `renameDir`'s
 * `oldPrefix === newPrefix` guard), or `null` on refusal. Pushes no undo
 * entry; callers manage the stack.
 */
async function applyDirRename(
  get: () => StudioState,
  oldPrefix: string,
  newPrefix: string,
): Promise<{
  moved: Array<{ oldPath: string; newPath: string }>;
  safe: boolean;
  introducedDiagnostics: RenameDiagnostic[];
} | null> {
  const state = get();
  const project = state._project;
  const documents = state._documents;
  if (!project || !documents) return null;

  const pendingDescription = `Renaming ${oldPrefix} → ${newPrefix}`;
  state.setStructuralOpPending(pendingDescription);
  let result: {
    moved: Array<{ oldPath: string; newPath: string }>;
    referrers: string[];
    safe: boolean;
    introducedDiagnostics: RenameDiagnostic[];
  };
  try {
    result = await project.renameDir(oldPrefix, newPrefix);
  } catch (e) {
    // renameDir validates (and throws) before mutating the session, so a
    // refused move — e.g. a destination collision, or an empty folder —
    // leaves every file untouched. Same reporting pattern as `applyRename`
    // (`docs/studio-shell-spec.md` §7.5).
    get()._notify?.({
      severity: "error",
      source: "binder",
      message: e instanceof Error ? e.message : `cannot rename ${oldPrefix}`,
    });
    return null;
  } finally {
    state.clearStructuralOpPending(pendingDescription);
  }

  // Re-key every moved file's open tabs/views in place, like `applyRename`
  // does for a single file.
  for (const { oldPath, newPath } of result.moved) {
    state.renameDocPath(oldPath, newPath);
  }
  for (const path of result.referrers) {
    documents.invalidateFile(path);
  }
  documents.triggerCompile();
  return {
    moved: result.moved,
    safe: result.safe,
    introducedDiagnostics: result.introducedDiagnostics,
  };
}

/**
 * Report a completed rename/move (issue #2918): reuses the existing binder
 * success-toast shape (severity/source/Undo action, §7.5) for both outcomes
 * of an applied move — `safe: false` is NOT a refusal (the op already ran,
 * same undo contract as a safe move; a refusal is reported at the `ok: false`
 * catch site above instead, with no undo entry) so this stays `warning`, not
 * `error` — matching the existing "skipped N read-only files" convention
 * just above (severity carries the caveat, the move still happened). This is
 * the notification FLOOR #2918 shipped: a post-move report, not a preflight
 * the user could still cancel — see that issue for why the Binder's rename
 * (no confirm dialog today, unlike delete's `pendingDelete`) doesn't get the
 * "will break N references" preflight the editor's inline (#323/#324) rename
 * has; that pattern lives on a dedicated widget this call site has no analog
 * of, and adding one is out of this fix's scope.
 */
function notifyMoveResult(
  get: () => StudioState,
  description: string,
  result: { safe: boolean; introducedDiagnostics: RenameDiagnostic[] },
): void {
  const n = result.introducedDiagnostics.length;
  const breakageSuffix = result.safe ? "" : ` (breaks ${n} reference${n === 1 ? "" : "s"})`;
  get()._notify?.({
    severity: result.safe ? "info" : "warning",
    source: "binder",
    message: `${description}${breakageSuffix}`,
    actions: [{ label: "Undo", commandId: "binder.undo" }],
  });
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
  const result = await applyRename(get, oldPath, newPath);
  if (result === null) return;

  const description = `${verb} ${oldPath} → ${newPath}`;
  set({
    undoStack: [
      ...get().undoStack,
      { kind: "rename", description, renames: [{ from: newPath, to: oldPath }] },
    ],
  });
  notifyMoveResult(get, description, result);
}
