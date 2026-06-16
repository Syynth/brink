import { describe, it, expect } from "vitest";
import { buildBinderTree } from "@brink/studio-ui";
import type { FileOutline } from "@brink/wasm-types";

const file = (path: string): FileOutline => ({ path, symbols: [] });

describe("buildBinderTree", () => {
  it("keeps root files (no slash) at the top level", () => {
    const tree = buildBinderTree([file("main.ink"), file("intro.ink")]);
    expect(tree.folders).toHaveLength(0);
    expect(tree.files.map((f) => f.path)).toEqual(["intro.ink", "main.ink"]); // sorted
  });

  it("groups files into a nested folder tree by path segments", () => {
    const tree = buildBinderTree([
      file("scenes/act1/intro.ink"),
      file("scenes/act1/fight.ink"),
      file("scenes/menu.ink"),
      file("main.ink"),
    ]);
    // root: one folder "scenes/" + the root file
    expect(tree.files.map((f) => f.path)).toEqual(["main.ink"]);
    expect(tree.folders.map((f) => f.key)).toEqual(["scenes/"]);
    const scenes = tree.folders[0]!;
    expect(scenes.name).toBe("scenes");
    expect(scenes.files.map((f) => f.path)).toEqual(["scenes/menu.ink"]);
    expect(scenes.folders.map((f) => f.key)).toEqual(["scenes/act1/"]);
    // nested folder sorts its files
    const act1 = scenes.folders[0]!;
    expect(act1.name).toBe("act1");
    expect(act1.files.map((f) => f.path)).toEqual([
      "scenes/act1/fight.ink",
      "scenes/act1/intro.ink",
    ]);
  });

  it("sorts folders by name within a level", () => {
    const tree = buildBinderTree([file("z/a.ink"), file("a/b.ink"), file("m/c.ink")]);
    expect(tree.folders.map((f) => f.name)).toEqual(["a", "m", "z"]);
  });
});
