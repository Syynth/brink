import { describe, it, expect } from "vitest";
import { computeReorder } from "@brink/studio-ui";

// computeReorder is the off-by-one-prone core of binder drag-reorder:
// given the current sibling order, the dragged subset, a reference row and
// a side, produce the new full order. The old code used a ±1 step and only
// the first selected item — these tests pin the order-based behavior.

describe("computeReorder", () => {
  const sibs = ["a", "b", "c", "d", "e"];

  it("moves a single item across multiple positions (drop after)", () => {
    // Drag "a" and drop after "d" → a lands between d and e.
    expect(computeReorder(sibs, ["a"], "d", "after")).toEqual(["b", "c", "d", "a", "e"]);
  });

  it("moves a single item across multiple positions (drop before)", () => {
    expect(computeReorder(sibs, ["a"], "d", "before")).toEqual(["b", "c", "a", "d", "e"]);
  });

  it("moves an item upward", () => {
    expect(computeReorder(sibs, ["e"], "b", "before")).toEqual(["a", "e", "b", "c", "d"]);
  });

  it("moves a multi-selection together, preserving relative order", () => {
    // Drag "a" and "c" (selection), drop after "d".
    expect(computeReorder(sibs, ["a", "c"], "d", "after")).toEqual(["b", "d", "a", "c", "e"]);
  });

  it("keeps dragged relative order regardless of selection arg order", () => {
    // Selection passed as [c, a] still resolves to document order a, c.
    expect(computeReorder(sibs, ["c", "a"], "d", "after")).toEqual(["b", "d", "a", "c", "e"]);
  });

  it("is a no-op when dropping onto a dragged item (drop onto self)", () => {
    expect(computeReorder(sibs, ["b"], "b", "before")).toEqual(sibs);
    expect(computeReorder(sibs, ["b", "c"], "c", "after")).toEqual(sibs);
  });

  it("is a no-op for an unknown reference", () => {
    expect(computeReorder(sibs, ["a"], "zzz", "after")).toEqual(sibs);
  });

  it("returns a permutation of the input (same multiset)", () => {
    const out = computeReorder(sibs, ["a", "e"], "c", "before");
    expect([...out].sort()).toEqual([...sibs].sort());
    expect(out).toHaveLength(sibs.length);
  });
});
