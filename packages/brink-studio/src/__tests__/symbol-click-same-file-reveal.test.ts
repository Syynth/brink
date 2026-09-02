/**
 * Knot/stitch click routing (#3356, RULED 2026-09-01): a symbol (knot or
 * stitch) navigation target whose file is already open as a whole-file tab
 * — anywhere, not only the active group — jumps in place instead of
 * opening a new tab.
 *
 * The bug: `TabTarget`'s symbol variant (`{ kind: "symbol", path, name,
 * start, end }`) becomes a DocumentRef with `docId = "path::name"`
 * (`inkFileRef` in `InkFileDocument.tsx`) — a key distinct from the plain
 * file tab's `docId = "path"`. `EditorGroupsState.openDocument`'s own
 * "reveal an existing tab" policy only matches by EXACT key, so it can
 * never recognize "the file this knot lives in is already open" — every
 * symbol click minted a fresh `path::name` fragment tab, even with the
 * whole file already open and active. `resolveSymbolFileTab` (`mount.tsx`)
 * is the fix: it searches every group for the file's WHOLE-FILE tab before
 * `setDocumentOpener` falls through to `openDocument`.
 *
 * `mountStudio` itself needs wasm + a compiled project to boot (see
 * `wasm-location.test.ts`'s note on why full-studio tests stop at
 * `initWasm`), so this exercises `resolveSymbolFileTab` directly — the
 * real function `setDocumentOpener`'s closure calls, not a reproduction of
 * it — plus the surrounding `EditorGroupsStore` mutations it triggers,
 * against a real store built the same way `mount.tsx` builds one.
 */

import { describe, it, expect } from "vitest";
import {
  createEditorGroupsStore,
  documentKey,
  findTab,
  type EditorGroup,
} from "@brink/studio-shell";
import { inkFileRef } from "@brink/studio-ui";
import { resolveSymbolFileTab } from "../mount.js";

function fileTab(path: string, pinned = true) {
  return { ref: inkFileRef({ kind: "file", path }), pinned };
}

const SYMBOL_TARGET = {
  kind: "symbol" as const,
  path: "story.brink",
  name: "haggle",
  start: 42,
  end: 90,
};

describe("resolveSymbolFileTab (#3356)", () => {
  it("finds the whole-file tab when it is the active tab in its group", () => {
    const groups: EditorGroup[] = [
      { id: "group-1", tabs: [fileTab("story.brink")], activeKey: documentKey(fileTab("story.brink").ref) },
    ];
    const found = resolveSymbolFileTab(groups, "story.brink");
    expect(found?.group.id).toBe("group-1");
    expect(found?.tab.ref.docId).toBe("story.brink");
  });

  it("finds the whole-file tab in a DIFFERENT, non-active group", () => {
    const groups: EditorGroup[] = [
      { id: "group-1", tabs: [fileTab("other.brink")], activeKey: null },
      { id: "group-2", tabs: [fileTab("story.brink")], activeKey: documentKey(fileTab("story.brink").ref) },
    ];
    const found = resolveSymbolFileTab(groups, "story.brink");
    expect(found?.group.id).toBe("group-2");
  });

  it("does not match a fragment tab of a DIFFERENT symbol in the same file", () => {
    // A "path::otherSymbol" tab is not the whole-file tab — falling through
    // to a fresh fragment open is still correct here (the ruling is about
    // the WHOLE FILE already being open, not any tab that merely names it).
    const groups: EditorGroup[] = [
      {
        id: "group-1",
        tabs: [{ ref: inkFileRef({ kind: "symbol", path: "story.brink", name: "haggle_2", start: 0, end: 1 }), pinned: true }],
        activeKey: null,
      },
    ];
    expect(resolveSymbolFileTab(groups, "story.brink")).toBeNull();
  });

  it("returns null when the file is not open anywhere", () => {
    const groups: EditorGroup[] = [{ id: "group-1", tabs: [], activeKey: null }];
    expect(resolveSymbolFileTab(groups, "story.brink")).toBeNull();
  });
});

describe("symbol click reveals in place — full store round trip (#3356)", () => {
  /** Reproduces `setDocumentOpener`'s symbol branch (`mount.tsx`) — the
   *  `documents.revealAt` call is the one piece needing a real
   *  DocumentSessions/CM6 view (already covered by `document-sessions.test.ts`'s
   *  `revealAt` tests), so it is stubbed here to isolate the routing this
   *  issue is about: does a tab get duplicated or not. */
  function openSymbol(
    store: ReturnType<typeof createEditorGroupsStore>,
    target: typeof SYMBOL_TARGET,
    pinned: boolean,
    revealAt: (path: string, offset: number) => void,
  ): void {
    const existing = resolveSymbolFileTab(store.getState().groups, target.path);
    if (existing !== null) {
      const key = documentKey(existing.tab.ref);
      store.getState().setActiveTab(existing.group.id, key);
      if (pinned && !existing.tab.pinned) store.getState().pinTab(existing.group.id, key);
      revealAt(target.path, target.start);
      return;
    }
    store
      .getState()
      .openDocument(inkFileRef({ kind: "symbol", path: target.path, name: target.name, start: target.start, end: target.end }), { pinned });
  }

  it("reveals in the active tab instead of opening a new one (same file, active)", () => {
    const store = createEditorGroupsStore();
    store.getState().openDocument(inkFileRef({ kind: "file", path: "story.brink" }));
    const reveals: Array<[string, number]> = [];

    openSymbol(store, SYMBOL_TARGET, false, (p, o) => reveals.push([p, o]));

    const group = store.getState().groups[0];
    expect(group.tabs).toHaveLength(1); // no fragment tab was minted
    expect(group.tabs[0].ref.docId).toBe("story.brink");
    expect(group.activeKey).toBe(documentKey(group.tabs[0].ref));
    expect(reveals).toEqual([["story.brink", 42]]);
  });

  it("focuses the file's tab in another group instead of duplicating (cross-group, same file)", () => {
    const store = createEditorGroupsStore();
    store.getState().openDocument(inkFileRef({ kind: "file", path: "other.brink" })); // group-1, focused
    store.getState().openDocument(inkFileRef({ kind: "file", path: "story.brink" }), { group: "split-right" }); // group-2
    store.getState().focusGroup("group-1"); // back on the file that ISN'T the target

    openSymbol(store, SYMBOL_TARGET, false, () => {});

    const totalTabs = store.getState().groups.flatMap((g) => g.tabs).length;
    expect(totalTabs).toBe(2); // still just the two whole-file tabs — no fragment tab
    expect(store.getState().focusedGroupId).toBe("group-2");
    const group2 = store.getState().groups.find((g) => g.id === "group-2")!;
    expect(group2.activeKey).toBe(documentKey(group2.tabs[0].ref));
  });

  it("still opens a fragment tab when the file isn't open anywhere (unchanged behavior)", () => {
    const store = createEditorGroupsStore();

    openSymbol(store, SYMBOL_TARGET, false, () => {});

    const group = store.getState().groups[0];
    expect(group.tabs).toHaveLength(1);
    expect(group.tabs[0].ref.docId).toBe("story.brink::haggle");
  });

  it("pins the existing tab when the reveal is a pinned open (parity with openDocument)", () => {
    const store = createEditorGroupsStore();
    store.getState().openDocument(inkFileRef({ kind: "file", path: "story.brink" }), { pinned: false }); // preview

    openSymbol(store, SYMBOL_TARGET, true, () => {});

    expect(store.getState().groups[0].tabs[0].pinned).toBe(true);
  });
});
