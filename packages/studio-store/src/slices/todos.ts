/**
 * TODOs panel view state — tag chips and grouping.
 *
 * Lives in the store for the same reason the Problems slice does: the
 * controls sit in the tool window's CHROME HEADER
 * (`ToolWindowDescriptor.actions`), which the shell renders in a different
 * subtree from the panel body. The two can only share state through a
 * store.
 *
 * **Tag selection is deliberately NOT persisted.** Tags are a property of
 * the project's own notes — `TODO(audio)` means nothing in the next project
 * — so a selection restored across a project switch would filter rows to a
 * tag that does not exist there, leaving an empty panel with no visible
 * cause. Grouping persists, because it is a reading preference rather than
 * a statement about content, exactly like the Problems panel's.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";

export interface TodosSlice {
  /** Selected tag chips. Empty = no tag filter, every note shows. */
  todosSelectedTags: readonly string[];
  /** Whether the chips row is revealed (the funnel button's state). */
  todosFilterOpen: boolean;
  /** Group notes into per-file sections. */
  todosGrouped: boolean;

  toggleTodoTag(tag: string): void;
  /** Drop every selected tag — the "show everything" reset. */
  clearTodoTags(): void;
  /** Toggle the chips row. Closing also clears the selection, so a hidden
   *  filter can never silently hide notes. */
  toggleTodosFilter(): void;
  toggleTodosGrouped(): void;
  /** Apply persisted preferences at boot (mount.tsx). */
  applyTodosPrefs(prefs: TodosPrefs): void;
  /** Injected persistence sink; null until the app binds it. */
  _persistTodosPrefs: ((prefs: TodosPrefs) => void) | null;
  setTodosPrefsSink(sink: (prefs: TodosPrefs) => void): void;
}

/** The persisted subset — grouping only; never the tag selection. */
export interface TodosPrefs {
  grouped: boolean;
}

export const TODOS_STORAGE_KEY = "brink-studio.todos.v1";

const DEFAULT_PREFS: TodosPrefs = { grouped: true };

/** Load persisted preferences. Never throws; defaults on anything odd. */
export function loadTodosPrefs(storage: Pick<Storage, "getItem">): TodosPrefs {
  let raw: string | null;
  try {
    raw = storage.getItem(TODOS_STORAGE_KEY);
  } catch {
    return DEFAULT_PREFS;
  }
  if (raw === null || raw === "") return DEFAULT_PREFS;
  try {
    const parsed = JSON.parse(raw) as { grouped?: unknown } | null;
    // Only an explicit `false` ungroups — a partial record keeps the
    // default, the same rule the Problems preferences use.
    return { grouped: parsed?.grouped !== false };
  } catch {
    return DEFAULT_PREFS;
  }
}

/** Persist preferences. Storage failures degrade to in-session. */
export function saveTodosPrefs(storage: Pick<Storage, "setItem">, prefs: TodosPrefs): void {
  try {
    storage.setItem(TODOS_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Quota/denied — the choice still applies for this session.
  }
}

export const createTodosSlice: StateCreator<StudioState, [], [], TodosSlice> = (set, get) => ({
  todosSelectedTags: [],
  todosFilterOpen: false,
  todosGrouped: true,

  _persistTodosPrefs: null,

  setTodosPrefsSink(sink) {
    set({ _persistTodosPrefs: sink });
  },

  applyTodosPrefs(prefs) {
    set({ todosGrouped: prefs.grouped });
  },

  toggleTodoTag(tag) {
    const current = get().todosSelectedTags;
    set({
      todosSelectedTags: current.includes(tag)
        ? current.filter((t) => t !== tag)
        : [...current, tag],
    });
  },

  clearTodoTags() {
    if (get().todosSelectedTags.length === 0) return;
    set({ todosSelectedTags: [] });
  },

  toggleTodosFilter() {
    const open = !get().todosFilterOpen;
    // Closing clears the selection: a filter that is hiding notes while its
    // own controls are off screen is a panel lying about what it holds.
    set({ todosFilterOpen: open, ...(open ? {} : { todosSelectedTags: [] }) });
  },

  toggleTodosGrouped() {
    const grouped = !get().todosGrouped;
    set({ todosGrouped: grouped });
    get()._persistTodosPrefs?.({ grouped });
  },
});
