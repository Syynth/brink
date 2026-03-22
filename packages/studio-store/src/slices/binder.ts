/**
 * Binder slice — collapsed state for the file/symbol tree.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";

export interface BinderSlice {
  collapsed: Set<string>;

  toggleCollapsed(key: string): void;
}

export const createBinderSlice: StateCreator<StudioState, [], [], BinderSlice> = (set, get) => ({
  collapsed: new Set<string>(),

  toggleCollapsed(key) {
    const next = new Set(get().collapsed);
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
    }
    set({ collapsed: next });
  },
});
