/**
 * Binder slice — collapsed state, multi-select, undo stack, and toast
 * for the file/symbol tree.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { MoveResult } from "@brink/wasm-types";

// ── Undo entry ──────────────────────────────────────────────────────

export interface UndoEntry {
  description: string;
  snapshots: Array<{ path: string; source: string }>;
}

// ── Slice interface ─────────────────────────────────────────────────

export interface BinderSlice {
  collapsed: Set<string>;
  selectedKeys: Set<string>;
  focusedKey: string | null;
  undoStack: UndoEntry[];
  toastMessage: string | null;
  toastUndoAction: (() => void) | null;

  toggleCollapsed(key: string): void;
  selectKey(key: string, multi: boolean): void;
  clearSelection(): void;
  setFocusedKey(key: string | null): void;
  applyMoveResult(
    result: MoveResult,
    description: string,
    affectedPaths: string[],
  ): Promise<void>;
  undo(): Promise<void>;
  dismissToast(): void;
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
  toastMessage: null,
  toastUndoAction: null,

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
    const stateManager = state._stateManager;
    const editorRef = state._editorRef;
    if (!project || !stateManager || !editorRef) return;

    const session = project.getSession();

    // 1. Snapshot current sources for undo
    const snapshots: Array<{ path: string; source: string }> = [];
    for (const path of affectedPaths) {
      const source = session.getFileSource(path);
      if (source != null) {
        snapshots.push({ path, source });
      }
    }

    // 2. Apply new_source to the target file (from result.path)
    if (result.new_source != null && result.path) {
      session.updateFile(result.path, result.new_source);
    }

    // 3. Apply cross_file_edits (if any)
    // Cross-file edits use file IDs — for now these are not common in
    // single-file operations, but the infrastructure is here.

    // 4. Push undo entry
    const undoStack = [...state.undoStack, { description, snapshots }];

    // 5. Invalidate editor states for affected files. invalidateFile clears
    //    view context and reloads the active view if it targets the path.
    for (const path of affectedPaths) {
      if (stateManager.invalidateFile) {
        stateManager.invalidateFile(path);
      }
    }

    // 6. Trigger recompile (refreshes outline)
    editorRef.triggerCompile();

    // 7. Set toast
    const self = get();
    set({
      undoStack,
      toastMessage: description,
      toastUndoAction: () => self.undo(),
    });
  },

  async undo() {
    const state = get();
    const project = state._project;
    const stateManager = state._stateManager;
    const editorRef = state._editorRef;
    if (!project || !stateManager || !editorRef) return;

    const stack = [...state.undoStack];
    const entry = stack.pop();
    if (!entry) return;

    const session = project.getSession();

    // Restore each snapshot
    for (const { path, source } of entry.snapshots) {
      session.updateFile(path, source);
    }

    // Invalidate editor states
    if (stateManager.invalidateFile) {
      for (const { path } of entry.snapshots) {
        stateManager.invalidateFile(path);
      }
    }

    // Trigger recompile
    editorRef.triggerCompile();

    set({
      undoStack: stack,
      toastMessage: `Undid: ${entry.description}`,
      toastUndoAction: null,
    });
  },

  dismissToast() {
    set({ toastMessage: null, toastUndoAction: null });
  },
});
