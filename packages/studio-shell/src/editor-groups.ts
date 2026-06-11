/**
 * @brink/studio-shell — editor groups store (docs/studio-shell-spec.md §7.8).
 *
 * The editor area hosts a flat, ordered list of vertical groups (columns),
 * each a tab strip over DocumentRefs. The shell owns this structure — tab
 * order, pin/preview state, the active tab per group, the focused group —
 * while document *content* belongs to the registered document components.
 *
 * Open/reveal policy: a plain open focuses an existing tab wherever it lives
 * (a document is open at most once unless the user explicitly asks for more);
 * duplicates are created only by explicit actions (`splitGroup`, opening into
 * an explicit target group). Preview semantics are generic tab behavior: at
 * most one unpinned (preview) tab per group, replaced in place by the next
 * preview open; editing or double-clicking pins.
 *
 * No persistence (future work, like dock layout pre-#88) and no nested grids
 * or horizontal splits (non-goals for now). Created inside ShellProvider as a
 * vanilla store — components read it via useEditorGroups(selector).
 */

import { createStore, type StoreApi } from "zustand/vanilla";
import { documentKey, type DocumentRef } from "./document.js";

export interface EditorTab {
  ref: DocumentRef;
  /** False = preview tab (italic label, replaced by the next preview open). */
  pinned: boolean;
}

export interface EditorGroup {
  /** Stable id ("group-1", …) — never reused within a store's lifetime. */
  id: string;
  tabs: EditorTab[];
  /** documentKey of the visible tab, or null for an empty group. */
  activeKey: string | null;
}

export interface OpenDocumentOptions {
  /**
   * Where to open: the focused group (default — focuses an existing tab in
   * ANY group first), "split-right" (a new group right of the focused one),
   * or an explicit group id. Explicit targets skip the any-group reveal —
   * they are the deliberate way to create a duplicate.
   */
  group?: "focused" | "split-right" | (string & {});
  /** Open pinned (default) or as the group's preview tab. */
  pinned?: boolean;
}

export interface EditorGroupsState {
  groups: EditorGroup[];
  focusedGroupId: string;
  /** Last splitter sizes in px per group id (not persisted). */
  groupSizes: Record<string, number>;
  /**
   * Maximized group (§5.4): this group temporarily takes the entire editor
   * area — other groups hide and ShellFrame collapses the open docks. Pure
   * presentation: no other state is touched, so restoring is just clearing
   * this field (dock open-state and group sizes come back untouched).
   */
  maximizedGroupId: string | null;

  /** Open per the reveal policy above; focuses the resulting group + tab. */
  openDocument(ref: DocumentRef, opts?: OpenDocumentOptions): void;
  /**
   * Close a tab. When a group's last tab closes the group collapses, unless
   * it is the only group (the editor area always has ≥ 1 group).
   */
  closeTab(groupId: string, key: string): void;
  /**
   * Move a tab to another group (keeps its pin state). If the target already
   * shows that document, the source tab is dropped and the existing tab is
   * focused instead — never a same-group duplicate.
   */
  moveTabToGroup(key: string, fromGroupId: string, toGroupId: string): void;
  /**
   * Split: duplicate the focused group's active tab into a new group
   * immediately to its right, and focus it (VS Code semantics — the explicit
   * way to view one document twice). An empty focused group splits into an
   * empty new group.
   */
  splitGroup(): void;
  focusGroup(groupId: string): void;
  /** Activate a tab; also focuses its group (clicking a tab focuses it). */
  setActiveTab(groupId: string, key: string): void;
  /** Pin a preview tab (double-click, or first edit via auto-pin). */
  pinTab(groupId: string, key: string): void;
  /** Remember a group's splitter size in px. */
  setGroupSize(groupId: string, px: number): void;
  /**
   * Maximize a group over the editor area, or restore (§5.4). With no id the
   * focused group maximizes; maximizing also focuses the group. Restoring
   * ignores the id — whichever group is maximized comes back down.
   */
  toggleMaximizeGroup(groupId?: string): void;
}

export type EditorGroupsStore = StoreApi<EditorGroupsState>;

/** Find a tab by document key across all groups (first group wins). */
export function findTab(
  groups: readonly EditorGroup[],
  key: string,
): { group: EditorGroup; tab: EditorTab } | null {
  for (const group of groups) {
    const tab = group.tabs.find((t) => documentKey(t.ref) === key);
    if (tab) return { group, tab };
  }
  return null;
}

/** The focused group (always exists — the store keeps ≥ 1 group). */
export function focusedGroup(state: EditorGroupsState): EditorGroup {
  return (
    state.groups.find((g) => g.id === state.focusedGroupId) ?? state.groups[0]
  );
}

/** The focused group's active tab, or null. */
export function focusedTab(state: EditorGroupsState): EditorTab | null {
  const group = focusedGroup(state);
  if (group.activeKey === null) return null;
  return group.tabs.find((t) => documentKey(t.ref) === group.activeKey) ?? null;
}

export function createEditorGroupsStore(): EditorGroupsStore {
  let nextGroupId = 2;
  const newGroup = (): EditorGroup => ({
    id: `group-${nextGroupId++}`,
    tabs: [],
    activeKey: null,
  });

  return createStore<EditorGroupsState>()((set, get) => ({
    groups: [{ id: "group-1", tabs: [], activeKey: null }],
    focusedGroupId: "group-1",
    groupSizes: {},
    maximizedGroupId: null,

    openDocument(ref, opts) {
      const pinned = opts?.pinned ?? true;
      const target = opts?.group ?? "focused";
      const key = documentKey(ref);

      set((s) => {
        // Plain opens reveal an existing tab wherever it lives (§7.8).
        if (target === "focused") {
          const existing = findTab(s.groups, key);
          if (existing) {
            const groups = s.groups.map((g) =>
              g.id === existing.group.id
                ? {
                    ...g,
                    activeKey: key,
                    tabs: g.tabs.map((t) =>
                      // A pinned open of an existing preview tab pins it.
                      documentKey(t.ref) === key && pinned && !t.pinned
                        ? { ...t, pinned: true }
                        : t,
                    ),
                  }
                : g,
            );
            return { groups, focusedGroupId: existing.group.id };
          }
        }

        // Resolve the target group, creating one for "split-right".
        let groups = s.groups;
        let groupId: string;
        if (target === "split-right") {
          const created = newGroup();
          const at = groups.findIndex((g) => g.id === s.focusedGroupId);
          groups = [...groups.slice(0, at + 1), created, ...groups.slice(at + 1)];
          groupId = created.id;
        } else if (target === "focused") {
          groupId = s.focusedGroupId;
        } else {
          groupId = groups.some((g) => g.id === target) ? target : s.focusedGroupId;
        }

        groups = groups.map((g) => {
          if (g.id !== groupId) return g;
          const existing = g.tabs.find((t) => documentKey(t.ref) === key);
          if (existing) {
            const tabs = g.tabs.map((t) =>
              documentKey(t.ref) === key && pinned && !t.pinned
                ? { ...t, pinned: true }
                : t,
            );
            return { ...g, tabs, activeKey: key };
          }
          const tab: EditorTab = { ref, pinned };
          if (!pinned) {
            // Preview opens replace the group's preview tab in place.
            const previewIdx = g.tabs.findIndex((t) => !t.pinned);
            if (previewIdx >= 0) {
              const tabs = [...g.tabs];
              tabs[previewIdx] = tab;
              return { ...g, tabs, activeKey: key };
            }
          }
          return { ...g, tabs: [...g.tabs, tab], activeKey: key };
        });

        return { groups, focusedGroupId: groupId };
      });
    },

    closeTab(groupId, key) {
      set((s) => {
        const group = s.groups.find((g) => g.id === groupId);
        if (!group) return {};
        const idx = group.tabs.findIndex((t) => documentKey(t.ref) === key);
        if (idx < 0) return {};

        const tabs = group.tabs.filter((_, i) => i !== idx);

        // Last tab gone: collapse the group (unless it is the only one).
        if (tabs.length === 0 && s.groups.length > 1) {
          const at = s.groups.findIndex((g) => g.id === groupId);
          const groups = s.groups.filter((g) => g.id !== groupId);
          const neighbor = groups[Math.max(0, at - 1)];
          const groupSizes = { ...s.groupSizes };
          delete groupSizes[groupId];
          return {
            groups,
            groupSizes,
            focusedGroupId:
              s.focusedGroupId === groupId ? neighbor.id : s.focusedGroupId,
            maximizedGroupId:
              s.maximizedGroupId === groupId ? null : s.maximizedGroupId,
          };
        }

        let activeKey = group.activeKey;
        if (activeKey === key) {
          const next = group.tabs[idx + 1] ?? group.tabs[idx - 1];
          activeKey = next ? documentKey(next.ref) : null;
        }
        const groups = s.groups.map((g) =>
          g.id === groupId ? { ...g, tabs, activeKey } : g,
        );
        return { groups };
      });
    },

    moveTabToGroup(key, fromGroupId, toGroupId) {
      if (fromGroupId === toGroupId) return;
      set((s) => {
        const from = s.groups.find((g) => g.id === fromGroupId);
        const to = s.groups.find((g) => g.id === toGroupId);
        if (!from || !to) return {};
        const moved = from.tabs.find((t) => documentKey(t.ref) === key);
        if (!moved) return {};

        const fromTabs = from.tabs.filter((t) => documentKey(t.ref) !== key);
        const toHasIt = to.tabs.some((t) => documentKey(t.ref) === key);

        let groups = s.groups.map((g) => {
          if (g.id === fromGroupId) {
            let activeKey = g.activeKey;
            if (activeKey === key) {
              const idx = g.tabs.findIndex((t) => documentKey(t.ref) === key);
              const next = g.tabs[idx + 1] ?? g.tabs[idx - 1];
              activeKey = next && documentKey(next.ref) !== key ? documentKey(next.ref) : null;
            }
            return { ...g, tabs: fromTabs, activeKey };
          }
          if (g.id === toGroupId) {
            // Target already shows this document: drop the duplicate.
            const tabs = toHasIt ? g.tabs : [...g.tabs, moved];
            return { ...g, tabs, activeKey: key };
          }
          return g;
        });

        let focusedGroupId = toGroupId;
        const groupSizes = { ...s.groupSizes };
        let maximizedGroupId = s.maximizedGroupId;
        if (fromTabs.length === 0 && groups.length > 1) {
          groups = groups.filter((g) => g.id !== fromGroupId);
          delete groupSizes[fromGroupId];
          if (maximizedGroupId === fromGroupId) maximizedGroupId = null;
        }
        return { groups, focusedGroupId, groupSizes, maximizedGroupId };
      });
    },

    splitGroup() {
      set((s) => {
        const at = s.groups.findIndex((g) => g.id === s.focusedGroupId);
        const source = s.groups[at] ?? s.groups[0];
        const created = newGroup();
        const active =
          source.activeKey !== null
            ? source.tabs.find((t) => documentKey(t.ref) === source.activeKey)
            : undefined;
        if (active) {
          created.tabs = [{ ...active }];
          created.activeKey = documentKey(active.ref);
        }
        const groups = [...s.groups.slice(0, at + 1), created, ...s.groups.slice(at + 1)];
        // Splitting while maximized restores first — the new group must be
        // visible, and a hidden split would be a silent no-op.
        return { groups, focusedGroupId: created.id, maximizedGroupId: null };
      });
    },

    focusGroup(groupId) {
      set((s) =>
        s.focusedGroupId !== groupId && s.groups.some((g) => g.id === groupId)
          ? { focusedGroupId: groupId }
          : {},
      );
    },

    setActiveTab(groupId, key) {
      set((s) => {
        const group = s.groups.find((g) => g.id === groupId);
        if (!group || !group.tabs.some((t) => documentKey(t.ref) === key)) return {};
        const groups =
          group.activeKey === key
            ? s.groups
            : s.groups.map((g) => (g.id === groupId ? { ...g, activeKey: key } : g));
        return { groups, focusedGroupId: groupId };
      });
    },

    pinTab(groupId, key) {
      set((s) => {
        const group = s.groups.find((g) => g.id === groupId);
        const tab = group?.tabs.find((t) => documentKey(t.ref) === key);
        if (!group || !tab || tab.pinned) return {};
        const groups = s.groups.map((g) =>
          g.id === groupId
            ? {
                ...g,
                tabs: g.tabs.map((t) =>
                  documentKey(t.ref) === key ? { ...t, pinned: true } : t,
                ),
              }
            : g,
        );
        return { groups };
      });
    },

    setGroupSize(groupId, px) {
      if (!Number.isFinite(px) || px <= 0) return;
      const rounded = Math.round(px);
      if (get().groupSizes[groupId] === rounded) return;
      set((s) => ({ groupSizes: { ...s.groupSizes, [groupId]: rounded } }));
    },

    toggleMaximizeGroup(groupId) {
      set((s) => {
        if (s.maximizedGroupId !== null) return { maximizedGroupId: null };
        const id = groupId ?? s.focusedGroupId;
        if (!s.groups.some((g) => g.id === id)) return {};
        return { maximizedGroupId: id, focusedGroupId: id };
      });
    },
  }));
}
