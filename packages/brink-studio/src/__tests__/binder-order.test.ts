/**
 * The .binder.json order sidecar's pure model (#3038 — compare
 * `docs/design/binder-v2/Order.dc.html`): parse self-heals, ordering is
 * listed-then-fallback (entry first, folders before files, alphabetical),
 * re-keying survives renames, removal drops subtrees.
 */
import { describe, expect, it } from "vitest";
import {
  EMPTY_BINDER_ORDER,
  addFolder,
  applyReorder,
  orderChildIds,
  parseBinderOrder,
  rekeyBinderOrder,
  removeFromBinderOrder,
  serializeBinderOrder,
  type BinderOrder,
} from "@brink/studio-store";

describe("parseBinderOrder", () => {
  it("round-trips through serialize", () => {
    const value: BinderOrder = {
      order: { "": ["main.ink", "story/"], "story/": ["b.ink", "a.ink"] },
      folders: ["drafts/"],
    };
    expect(parseBinderOrder(serializeBinderOrder(value))).toEqual(value);
  });
  it("self-heals corrupt/missing content to the fallback", () => {
    expect(parseBinderOrder(null)).toEqual(EMPTY_BINDER_ORDER);
    expect(parseBinderOrder("{nope")).toEqual(EMPTY_BINDER_ORDER);
    expect(parseBinderOrder('{"order": 3, "folders": "x"}')).toEqual(EMPTY_BINDER_ORDER);
    // Partial junk: bad entries dropped, good ones kept.
    expect(
      parseBinderOrder('{"order": {"": ["a.ink", 5]}, "folders": ["ok/", "not-a-folder"]}'),
    ).toEqual({ order: { "": ["a.ink"] }, folders: ["ok/"] });
  });
});

describe("orderChildIds", () => {
  const children = ["b.ink", "a.ink", "menus/", "codetta.ink", "clues/"];
  it("falls back to entry-first, folders-before-files, alphabetical", () => {
    expect(orderChildIds("", children, EMPTY_BINDER_ORDER, "codetta.ink")).toEqual([
      "codetta.ink",
      "clues/",
      "menus/",
      "a.ink",
      "b.ink",
    ]);
  });
  it("listed ids come first in saved order; unlisted follow by the fallback", () => {
    const order: BinderOrder = { order: { "": ["menus/", "b.ink"] }, folders: [] };
    expect(orderChildIds("", children, order, "codetta.ink")).toEqual([
      "menus/",
      "b.ink",
      "codetta.ink",
      "clues/",
      "a.ink",
    ]);
  });
  it("ignores stale listed ids that no longer exist", () => {
    const order: BinderOrder = { order: { "": ["gone.ink", "a.ink"] }, folders: [] };
    expect(orderChildIds("", children, order, null)[0]).toBe("a.ink");
  });
});

describe("rekey / remove / reorder", () => {
  const base: BinderOrder = {
    order: { "": ["story/", "main.ink"], "story/": ["act2/", "x.ink"], "story/act2/": ["y.ink"] },
    folders: ["story/empty/"],
  };
  it("rekeys a folder rename across keys, child ids, and the folder registry", () => {
    const next = rekeyBinderOrder(base, "story/", "chapters/");
    expect(next.order[""]).toEqual(["chapters/", "main.ink"]);
    expect(next.order["chapters/"]).toEqual(["act2/", "x.ink"]);
    expect(next.order["chapters/act2/"]).toEqual(["y.ink"]);
    expect(next.folders).toEqual(["chapters/empty/"]);
  });
  it("rekeys a single file rename", () => {
    const next = rekeyBinderOrder(base, "main.ink", "start.ink");
    expect(next.order[""]).toEqual(["story/", "start.ink"]);
  });
  it("removes a folder subtree everywhere", () => {
    const next = removeFromBinderOrder(base, "story/");
    expect(next.order[""]).toEqual(["main.ink"]);
    expect(next.order["story/"]).toBeUndefined();
    expect(next.order["story/act2/"]).toBeUndefined();
    expect(next.folders).toEqual([]);
  });
  it("applyReorder replaces the container's full list; addFolder dedups", () => {
    expect(applyReorder(base, "", ["main.ink", "story/"]).order[""]).toEqual([
      "main.ink",
      "story/",
    ]);
    expect(addFolder(base, "story/empty/")).toBe(base);
    expect(addFolder(base, "new/").folders).toContain("new/");
    expect(addFolder(base, "not-a-folder")).toBe(base);
  });
});
