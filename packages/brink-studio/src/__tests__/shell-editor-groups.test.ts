/**
 * @brink/studio-shell unit tests — editor groups + document API (issue #90,
 * spec §7.8): the document-type registry and the editor-groups store
 * (open/reveal policy, preview-pin semantics, split-duplicate, move,
 * collapse, focus, commands).
 */

import { describe, expect, it, beforeEach } from "vitest";
import {
  CommandRegistry,
  DocumentTypeRegistry,
  createEditorGroupsStore,
  documentKey,
  focusedGroup,
  focusedTab,
  registerEditorGroupCommands,
  type DocumentRef,
  type EditorGroupsStore,
} from "@brink/studio-shell";

function ref(docId: string, title = docId): DocumentRef {
  return { typeId: "ink-file", docId, title };
}

const MAIN = ref("main.ink");
const OTHER = ref("other.ink");
const THIRD = ref("third.ink");
const KEY_MAIN = documentKey(MAIN);
const KEY_OTHER = documentKey(OTHER);

// ── Document-type registry ─────────────────────────────────────────

describe("DocumentTypeRegistry", () => {
  const Component = () => null;

  it("registers, lists in order, and unregisters", () => {
    const registry = new DocumentTypeRegistry();
    const dispose = registry.register({ id: "ink-file", component: Component });
    registry.register({ id: "compiled-output", component: Component });

    expect(registry.list().map((d) => d.id)).toEqual(["ink-file", "compiled-output"]);
    expect(registry.get("ink-file")?.component).toBe(Component);

    dispose();
    expect(registry.get("ink-file")).toBeUndefined();
    expect(registry.list().map((d) => d.id)).toEqual(["compiled-output"]);
  });

  it("rejects duplicate ids and the host-reserved prefix", () => {
    const registry = new DocumentTypeRegistry();
    registry.register({ id: "ink-file", component: Component });
    expect(() => registry.register({ id: "ink-file", component: Component })).toThrow(
      /duplicate/,
    );
    expect(() =>
      registry.register({ id: "host.acme.panel", component: Component }),
    ).toThrow(/reserved/);
  });

  it("returns undefined for unknown types (EditorArea renders a placeholder)", () => {
    const registry = new DocumentTypeRegistry();
    expect(registry.get("nope")).toBeUndefined();
  });

  it("notifies change listeners", () => {
    const registry = new DocumentTypeRegistry();
    let calls = 0;
    const unsubscribe = registry.onDidChange(() => calls++);
    const dispose = registry.register({ id: "a", component: Component });
    dispose();
    unsubscribe();
    registry.register({ id: "b", component: Component });
    expect(calls).toBe(2);
  });
});

// ── Editor groups store ────────────────────────────────────────────

describe("editor groups store", () => {
  let store: EditorGroupsStore;

  beforeEach(() => {
    store = createEditorGroupsStore();
  });

  it("starts with one empty focused group", () => {
    const s = store.getState();
    expect(s.groups).toHaveLength(1);
    expect(s.focusedGroupId).toBe(s.groups[0].id);
    expect(s.groups[0].tabs).toHaveLength(0);
    expect(s.groups[0].activeKey).toBeNull();
  });

  describe("openDocument", () => {
    it("opens pinned in the focused group and activates", () => {
      store.getState().openDocument(MAIN);
      const g = focusedGroup(store.getState());
      expect(g.tabs.map((t) => documentKey(t.ref))).toEqual([KEY_MAIN]);
      expect(g.activeKey).toBe(KEY_MAIN);
      expect(g.tabs[0].pinned).toBe(true);
    });

    it("preview opens replace the group's preview tab in place", () => {
      store.getState().openDocument(MAIN, { pinned: false });
      store.getState().openDocument(OTHER, { pinned: false });
      const g = focusedGroup(store.getState());
      expect(g.tabs.map((t) => documentKey(t.ref))).toEqual([KEY_OTHER]);
      expect(g.tabs[0].pinned).toBe(false);
    });

    it("keeps at most one preview tab per group; pinned tabs are untouched", () => {
      store.getState().openDocument(MAIN, { pinned: true });
      store.getState().openDocument(OTHER, { pinned: false });
      store.getState().openDocument(THIRD, { pinned: false });
      const g = focusedGroup(store.getState());
      expect(g.tabs.map((t) => documentKey(t.ref))).toEqual([
        KEY_MAIN,
        documentKey(THIRD),
      ]);
      expect(g.tabs.filter((t) => !t.pinned)).toHaveLength(1);
    });

    it("a pinned open of an existing preview tab pins it (no duplicate)", () => {
      store.getState().openDocument(MAIN, { pinned: false });
      store.getState().openDocument(MAIN, { pinned: true });
      const g = focusedGroup(store.getState());
      expect(g.tabs).toHaveLength(1);
      expect(g.tabs[0].pinned).toBe(true);
    });

    it("reveal policy: plain open focuses an existing tab in ANY group", () => {
      store.getState().openDocument(MAIN);
      store.getState().splitGroup(); // duplicate into group 2, focused
      store.getState().openDocument(OTHER); // opens in group 2
      const group1 = store.getState().groups[0];
      store.getState().focusGroup(store.getState().groups[1].id);

      // OTHER lives in group 2 only; MAIN in both. Opening a doc that lives
      // only in group 1 must focus group 1, not duplicate into group 2.
      store.getState().closeTab(store.getState().groups[1].id, KEY_MAIN);
      store.getState().openDocument(MAIN);
      expect(store.getState().focusedGroupId).toBe(group1.id);
      expect(focusedTab(store.getState())?.ref.docId).toBe("main.ink");
      // No new tab appeared anywhere.
      const totalMain = store
        .getState()
        .groups.flatMap((g) => g.tabs)
        .filter((t) => documentKey(t.ref) === KEY_MAIN);
      expect(totalMain).toHaveLength(1);
    });

    it("explicit group target can create a cross-group duplicate", () => {
      store.getState().openDocument(MAIN);
      store.getState().openDocument(OTHER, { group: "split-right" });
      const g2 = store.getState().focusedGroupId;

      // MAIN lives only in group 1; explicitly targeting group 2 duplicates
      // (the deliberate action), where a plain open would have revealed.
      store.getState().openDocument(MAIN, { group: g2 });
      const copies = store
        .getState()
        .groups.flatMap((g) => g.tabs)
        .filter((t) => documentKey(t.ref) === KEY_MAIN);
      expect(copies).toHaveLength(2);

      // But never a same-group duplicate: repeating the open re-activates.
      store.getState().openDocument(MAIN, { group: g2 });
      expect(
        store
          .getState()
          .groups.find((g) => g.id === g2)!
          .tabs.filter((t) => documentKey(t.ref) === KEY_MAIN),
      ).toHaveLength(1);
    });

    it("split-right opens into a new group right of the focused one", () => {
      store.getState().openDocument(MAIN);
      const first = store.getState().focusedGroupId;
      store.getState().openDocument(OTHER, { group: "split-right" });
      const s = store.getState();
      expect(s.groups).toHaveLength(2);
      expect(s.groups[0].id).toBe(first);
      expect(s.focusedGroupId).toBe(s.groups[1].id);
      expect(focusedTab(s)?.ref.docId).toBe("other.ink");
    });
  });

  describe("splitGroup", () => {
    it("duplicates the focused group's active tab into a new right group", () => {
      store.getState().openDocument(MAIN);
      store.getState().openDocument(OTHER);
      store.getState().setActiveTab(store.getState().groups[0].id, KEY_MAIN);
      store.getState().splitGroup();

      const s = store.getState();
      expect(s.groups).toHaveLength(2);
      expect(s.focusedGroupId).toBe(s.groups[1].id);
      expect(s.groups[1].tabs.map((t) => documentKey(t.ref))).toEqual([KEY_MAIN]);
      expect(s.groups[1].activeKey).toBe(KEY_MAIN);
      // Source keeps its tabs.
      expect(s.groups[0].tabs).toHaveLength(2);
    });

    it("splits an empty group into an empty group", () => {
      store.getState().splitGroup();
      const s = store.getState();
      expect(s.groups).toHaveLength(2);
      expect(s.groups[1].tabs).toHaveLength(0);
    });
  });

  describe("closeTab", () => {
    it("activates a neighbor when the active tab closes", () => {
      store.getState().openDocument(MAIN);
      store.getState().openDocument(OTHER);
      store.getState().openDocument(THIRD);
      const g = store.getState().groups[0].id;
      store.getState().setActiveTab(g, KEY_OTHER);
      store.getState().closeTab(g, KEY_OTHER);
      expect(store.getState().groups[0].activeKey).toBe(documentKey(THIRD));
    });

    it("collapses a group when its last tab closes (≥1 group invariant)", () => {
      store.getState().openDocument(MAIN);
      store.getState().splitGroup();
      const g2 = store.getState().focusedGroupId;
      store.getState().closeTab(g2, KEY_MAIN);

      const s = store.getState();
      expect(s.groups).toHaveLength(1);
      expect(s.focusedGroupId).toBe(s.groups[0].id);

      // The only group never collapses — it just empties.
      store.getState().closeTab(s.groups[0].id, KEY_MAIN);
      expect(store.getState().groups).toHaveLength(1);
      expect(store.getState().groups[0].tabs).toHaveLength(0);
    });
  });

  describe("moveTabToGroup", () => {
    it("moves a tab (keeping pin state) and focuses the target", () => {
      store.getState().openDocument(MAIN, { pinned: false });
      store.getState().openDocument(OTHER);
      store.getState().splitGroup();
      const [g1, g2] = store.getState().groups.map((g) => g.id);

      store.getState().moveTabToGroup(KEY_MAIN, g1, g2);
      const s = store.getState();
      expect(s.groups.find((g) => g.id === g1)!.tabs.map((t) => t.ref.docId)).toEqual([
        "other.ink",
      ]);
      const movedTab = s.groups
        .find((g) => g.id === g2)!
        .tabs.find((t) => documentKey(t.ref) === KEY_MAIN)!;
      expect(movedTab.pinned).toBe(false);
      expect(s.focusedGroupId).toBe(g2);
      expect(s.groups.find((g) => g.id === g2)!.activeKey).toBe(KEY_MAIN);
    });

    it("drops the duplicate when the target already shows the document", () => {
      store.getState().openDocument(MAIN);
      store.getState().splitGroup(); // both groups show MAIN
      const [g1, g2] = store.getState().groups.map((g) => g.id);
      store.getState().openDocument(OTHER, { group: g1 });

      store.getState().moveTabToGroup(KEY_MAIN, g1, g2);
      const s = store.getState();
      const g2Tabs = s.groups.find((g) => g.id === g2)!.tabs;
      expect(g2Tabs.filter((t) => documentKey(t.ref) === KEY_MAIN)).toHaveLength(1);
      expect(
        s.groups.find((g) => g.id === g1)!.tabs.some((t) => documentKey(t.ref) === KEY_MAIN),
      ).toBe(false);
    });

    it("collapses the source group when it empties", () => {
      store.getState().openDocument(MAIN);
      store.getState().openDocument(OTHER, { group: "split-right" });
      const [g1, g2] = store.getState().groups.map((g) => g.id);
      store.getState().moveTabToGroup(KEY_OTHER, g2, g1);
      const s = store.getState();
      expect(s.groups).toHaveLength(1);
      expect(s.groups[0].id).toBe(g1);
    });
  });

  describe("focus & pin", () => {
    it("setActiveTab activates and focuses the group", () => {
      store.getState().openDocument(MAIN);
      store.getState().openDocument(OTHER, { group: "split-right" });
      const g1 = store.getState().groups[0].id;
      store.getState().setActiveTab(g1, KEY_MAIN);
      expect(store.getState().focusedGroupId).toBe(g1);
    });

    it("pinTab pins a preview tab", () => {
      store.getState().openDocument(MAIN, { pinned: false });
      const g = store.getState().groups[0].id;
      store.getState().pinTab(g, KEY_MAIN);
      expect(store.getState().groups[0].tabs[0].pinned).toBe(true);
    });

    it("focusGroup ignores unknown ids", () => {
      const before = store.getState().focusedGroupId;
      store.getState().focusGroup("nope");
      expect(store.getState().focusedGroupId).toBe(before);
    });
  });
});

// ── Editor-group commands ──────────────────────────────────────────

describe("editor-group commands", () => {
  it("editor.split duplicates; move/focus commands gate on layout", () => {
    const commands = new CommandRegistry();
    const store = createEditorGroupsStore();
    registerEditorGroupCommands(commands, store);

    expect(commands.get("editor.split")?.keybinding).toBe("Mod-\\");
    expect(commands.isEnabled("editor.moveTabRight")).toBe(false);
    expect(commands.isEnabled("editor.moveTabLeft")).toBe(false);
    expect(commands.isEnabled("editor.focusNextGroup")).toBe(false);

    store.getState().openDocument(MAIN);
    store.getState().openDocument(OTHER);
    expect(commands.isEnabled("editor.moveTabRight")).toBe(true);

    commands.dispatch("editor.split");
    expect(store.getState().groups).toHaveLength(2);
    expect(commands.isEnabled("editor.focusNextGroup")).toBe(true);
    expect(commands.isEnabled("editor.moveTabLeft")).toBe(true);

    // Focus cycles.
    const focused = store.getState().focusedGroupId;
    commands.dispatch("editor.focusNextGroup");
    expect(store.getState().focusedGroupId).not.toBe(focused);

    // moveTabLeft from group 2 merges back; the duplicate is dropped and
    // group 2 collapses.
    store.getState().focusGroup(store.getState().groups[1].id);
    commands.dispatch("editor.moveTabLeft");
    expect(store.getState().groups).toHaveLength(1);
  });

  it("editor.moveTabRight without a right neighbor splits-then-closes (a move)", () => {
    const commands = new CommandRegistry();
    const store = createEditorGroupsStore();
    registerEditorGroupCommands(commands, store);

    store.getState().openDocument(MAIN);
    store.getState().openDocument(OTHER);
    commands.dispatch("editor.moveTabRight");

    const s = store.getState();
    expect(s.groups).toHaveLength(2);
    expect(s.groups[0].tabs.map((t) => t.ref.docId)).toEqual(["main.ink"]);
    expect(s.groups[1].tabs.map((t) => t.ref.docId)).toEqual(["other.ink"]);
  });
});
