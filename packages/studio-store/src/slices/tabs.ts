/**
 * Tabs slice — open tabs, active tab, and file list management.
 *
 * Delegates tab operations to the EditorStateManager which owns the
 * underlying CM6 EditorState instances and wasm session coordination.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { TabInfo, TabTarget } from "../types.js";

export interface TabsSlice {
  tabs: TabInfo[];
  activeTabId: string;
  files: string[];

  openTab(target: TabTarget, pinned: boolean): Promise<void>;
  closeTab(id: string): Promise<boolean>;
  pinTab(id: string): void;
  pinActiveTab(): void;
  addFile(name: string): Promise<void>;
}

export const createTabsSlice: StateCreator<StudioState, [], [], TabsSlice> = (set, get) => ({
  tabs: [],
  activeTabId: "",
  files: [],

  async openTab(target, pinned) {
    const mgr = get()._stateManager;
    if (!mgr) return;
    await mgr.openTab(target, pinned);
    syncTabsFromManager(set, mgr);
  },

  async closeTab(id) {
    const mgr = get()._stateManager;
    if (!mgr) return false;
    const closed: boolean = await mgr.closeTab(id);
    if (closed) {
      syncTabsFromManager(set, mgr);
    }
    return closed;
  },

  pinTab(id) {
    const mgr = get()._stateManager;
    if (!mgr) return;
    mgr.pinTab(id);
    syncTabsFromManager(set, mgr);
  },

  pinActiveTab() {
    const mgr = get()._stateManager;
    if (!mgr) return;
    mgr.pinActiveTab();
    syncTabsFromManager(set, mgr);
  },

  async addFile(name) {
    const mgr = get()._stateManager;
    if (!mgr) return;
    await mgr.addFile(name);
    syncTabsFromManager(set, mgr);
  },
});

// ── Helpers ──────────────────────────────────────────────────────────

type SetFn = (partial: Partial<Pick<StudioState, "tabs" | "activeTabId" | "files">>) => void;

/**
 * Pull the current tab/file state from the EditorStateManager into Zustand.
 *
 * The manager is the source of truth for tabs; we mirror into the store
 * so React components can subscribe to changes.
 */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function syncTabsFromManager(set: SetFn, mgr: any): void {
  const tabs: TabInfo[] = [...mgr.getTabs()];
  const activeTab = mgr.getActiveTab();
  const files: string[] = mgr.files();
  set({
    tabs,
    activeTabId: activeTab.id,
    files,
  });
}
