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
      // Span-qualified since #3136: a name is not unique within a file.
      "sym:main.ink:warden:40",
      "sym:main.ink:warden.turn:60",
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
    expect(ranked[0]!.key).toBe("sym:main.ink:warden.turn:60");
  });

  it("empty query returns everything in input order", () => {
    expect(rankQuickPickItems(items, " ").map((i) => i.key)).toEqual(items.map((i) => i.key));
  });

  it("drops non-matches", () => {
    expect(rankQuickPickItems(items, "zzz")).toEqual([]);
  });
});

// ── The manuscript only, and unique keys (#3136) ────────────────────

describe("buildQuickOpenItems excludes what the Binder excludes", () => {
  const WITH_LIBRARY: FileOutline[] = [
    ...OUTLINE,
    // A mounted std/ file, as the outline actually carries it.
    {
      path: "std/conventions/screenplay.brink",
      mounted: true,
      symbols: [
        {
          name: "scene_entered",
          kind: "function",
          start: 10,
          end: 23,
          full_start: 10,
          full_end: 40,
          children: [],
        },
      ],
    } as FileOutline,
    { path: "brink.toml", symbols: [] },
  ];

  it("omits mounted library files and brink.toml", () => {
    // They are not places you navigate to while writing, and the Binder tree
    // and Continuous view both already filter them out.
    const items = buildQuickOpenItems(WITH_LIBRARY);
    expect(items.some((i) => i.file.startsWith("std/"))).toBe(false);
    expect(items.some((i) => i.file === "brink.toml")).toBe(false);
    expect(items.some((i) => i.file === "main.ink")).toBe(true);
  });

  it("gives every item a unique key even when a name repeats", () => {
    // The reported symptom was a React duplicate-key warning, which silently
    // drops or duplicates rows rather than erroring — so the invariant is
    // worth asserting directly, not just via the filter that hid the case.
    const REPEATED: FileOutline[] = [
      {
        path: "a.ink",
        symbols: [
          { name: "beat", kind: "knot", start: 0, end: 4, full_start: 0, full_end: 9, children: [] },
          { name: "beat", kind: "knot", start: 50, end: 54, full_start: 50, full_end: 60, children: [] },
        ],
      },
    ];
    const keys = buildQuickOpenItems(REPEATED).map((i) => i.key);
    expect(new Set(keys).size).toBe(keys.length);
  });
});
