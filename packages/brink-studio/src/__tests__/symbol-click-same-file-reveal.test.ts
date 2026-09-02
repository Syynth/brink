/**
 * Knot/stitch click routing (#3356, RULED 2026-09-01): a symbol (knot or
 * stitch) NAVIGATION target (`pinned === false`, a plain click) whose file
 * is already open as a whole-file tab — anywhere, not only the active
 * group — jumps in place instead of opening a new tab. A PINNED
 * (double-click) open is excluded from this reveal and keeps minting or
 * focusing the `path::name` fragment tab exactly as before this fix —
 * docs/studio-shell-spec.md §7.8's Fragment⇄file overlap is a first-class
 * case, not something a navigation-open fix gets to retire.
 *
 * The bug: `TabTarget`'s symbol variant (`{ kind: "symbol", path, name,
 * start, end }`) becomes a DocumentRef with `docId = "path::name"`
 * (`inkFileRef` in `InkFileDocument.tsx`) — a key distinct from the plain
 * file tab's `docId = "path"`. `EditorGroupsState.openDocument`'s own
 * "reveal an existing tab" policy only matches by EXACT key, so it can
 * never recognize "the file this knot lives in is already open" — every
 * symbol click minted a fresh `path::name` fragment tab, even with the
 * whole file already open and active. `resolveSymbolFileTab` (`mount.tsx`)
 * finds that whole-file tab; `openSymbolTarget` (`mount.tsx`) is the routing
 * decision itself — gates on `pinned`, reveals in place, and reports back
 * whether it handled the open.
 *
 * `mountStudio` itself needs wasm + a compiled project to boot (see
 * `wasm-location.test.ts`'s note on why full-studio tests stop at
 * `initWasm`), so this exercises `resolveSymbolFileTab` and
 * `openSymbolTarget` directly — the exact functions `setDocumentOpener`'s
 * closure calls, not a reproduction of them — against a real store built
 * the same way `mount.tsx` builds one.
 */

import { describe, it, expect } from "vitest";
import {
  createEditorGroupsStore,
  documentKey,
  findTab,
  type EditorGroup,
} from "@brink/studio-shell";
import { inkFileRef } from "@brink/studio-ui";
import { resolveSymbolFileTab, openSymbolTarget } from "../mount.js";

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

describe("openSymbolTarget — full store round trip (#3356)", () => {
  /** The production fallback: exactly what `setDocumentOpener` does when
   *  `openSymbolTarget` returns `false` — mint/focus the `path::name`
   *  fragment tab. Exercised here (not reproduced) via the real
   *  `openDocument`, so a broken `openSymbolTarget` gate is caught the same
   *  way the production call site would catch it. */
  function fallbackOpen(
    store: ReturnType<typeof createEditorGroupsStore>,
    target: typeof SYMBOL_TARGET,
    pinned: boolean,
  ): void {
    store
      .getState()
      .openDocument(
        inkFileRef({ kind: "symbol", path: target.path, name: target.name, start: target.start, end: target.end }),
        { pinned },
      );
  }

  it("reveals in the active tab instead of opening a new one (same file, active, navigation open)", () => {
    const store = createEditorGroupsStore();
    store.getState().openDocument(inkFileRef({ kind: "file", path: "story.brink" }));
    const reveals: Array<[string, number]> = [];

    const handled = openSymbolTarget(store, SYMBOL_TARGET, false, (p, o) => reveals.push([p, o]));
    if (!handled) fallbackOpen(store, SYMBOL_TARGET, false);

    expect(handled).toBe(true);
    const group = store.getState().groups[0];
    expect(group.tabs).toHaveLength(1); // no fragment tab was minted
    expect(group.tabs[0].ref.docId).toBe("story.brink");
    expect(group.activeKey).toBe(documentKey(group.tabs[0].ref));
    expect(reveals).toEqual([["story.brink", 42]]);
  });

  it("focuses the file's tab in another group instead of duplicating (cross-group, same file, navigation open)", () => {
    const store = createEditorGroupsStore();
    store.getState().openDocument(inkFileRef({ kind: "file", path: "other.brink" })); // group-1, focused
    store.getState().openDocument(inkFileRef({ kind: "file", path: "story.brink" }), { group: "split-right" }); // group-2
    store.getState().focusGroup("group-1"); // back on the file that ISN'T the target

    const handled = openSymbolTarget(store, SYMBOL_TARGET, false, () => {});
    if (!handled) fallbackOpen(store, SYMBOL_TARGET, false);

    expect(handled).toBe(true);
    const totalTabs = store.getState().groups.flatMap((g) => g.tabs).length;
    expect(totalTabs).toBe(2); // still just the two whole-file tabs — no fragment tab
    expect(store.getState().focusedGroupId).toBe("group-2");
    const group2 = store.getState().groups.find((g) => g.id === "group-2")!;
    expect(group2.activeKey).toBe(documentKey(group2.tabs[0].ref));
  });

  it("still opens a fragment tab when the file isn't open anywhere (unchanged behavior)", () => {
    const store = createEditorGroupsStore();

    const handled = openSymbolTarget(store, SYMBOL_TARGET, false, () => {});
    if (!handled) fallbackOpen(store, SYMBOL_TARGET, false);

    expect(handled).toBe(false);
    const group = store.getState().groups[0];
    expect(group.tabs).toHaveLength(1);
    expect(group.tabs[0].ref.docId).toBe("story.brink::haggle");
  });

  it("a PINNED (double-click) open never reveals in place, even when the file is already open — mints the fragment tab instead", () => {
    const store = createEditorGroupsStore();
    store.getState().openDocument(inkFileRef({ kind: "file", path: "story.brink" }), { pinned: false }); // preview
    const reveals: Array<[string, number]> = [];

    const handled = openSymbolTarget(store, SYMBOL_TARGET, true, (p, o) => reveals.push([p, o]));
    if (!handled) fallbackOpen(store, SYMBOL_TARGET, true);

    expect(handled).toBe(false); // gated: pinned opens don't reveal
    expect(reveals).toEqual([]);
    const tabs = store.getState().groups[0].tabs;
    expect(tabs.map((t) => t.ref.docId)).toEqual(["story.brink", "story.brink::haggle"]);
    expect(tabs[1].pinned).toBe(true); // the fragment tab itself is pinned, per the pinned open
  });
});
