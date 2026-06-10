/**
 * Quick-open + QuickPick unit tests (shell issue 2.4, spec §6).
 */

import { describe, expect, it } from "vitest";
import { rankQuickPickItems } from "@brink/studio-shell";
import { buildQuickOpenItems } from "@brink/studio-ui";
import type { FileOutline } from "@brink/wasm-types";

const OUTLINE: FileOutline[] = [
  {
    path: "main.ink",
    symbols: [
      {
        name: "warden",
        kind: "knot",
        start: 40,
        end: 46,
        full_start: 40,
        full_end: 120,
        children: [
          {
            name: "turn",
            kind: "stitch",
            start: 60,
            end: 64,
            full_start: 60,
            full_end: 90,
            children: [],
          },
        ],
      },
    ],
  },
  { path: "extra.ink", symbols: [] },
];

describe("buildQuickOpenItems", () => {
  it("flattens files and symbols with qualified names, deterministic order", () => {
    const items = buildQuickOpenItems(OUTLINE);
    expect(items.map((i) => i.key)).toEqual([
      "file:main.ink",
      "sym:main.ink:warden",
      "sym:main.ink:warden.turn",
      "file:extra.ink",
    ]);
    const stitch = items[2]!;
    expect(stitch.title).toBe("warden.turn");
    expect(stitch.span).toEqual({ start: 60, end: 64 });
    expect(stitch.detail).toBe("stitch · main.ink");
  });

  it("file items target offset 0", () => {
    const items = buildQuickOpenItems(OUTLINE);
    expect(items[0]).toMatchObject({ file: "main.ink", span: { start: 0, end: 0 } });
  });
});

describe("rankQuickPickItems", () => {
  const items = buildQuickOpenItems(OUTLINE);

  it("ranks compact title matches first and matches searchText", () => {
    const ranked = rankQuickPickItems(items, "turn");
    expect(ranked[0]!.key).toBe("sym:main.ink:warden.turn");
  });

  it("empty query returns everything in input order", () => {
    expect(rankQuickPickItems(items, " ").map((i) => i.key)).toEqual(items.map((i) => i.key));
  });

  it("drops non-matches", () => {
    expect(rankQuickPickItems(items, "zzz")).toEqual([]);
  });
});
