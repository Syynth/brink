/**
 * Squarified treemap layout (#3339 Size view).
 *
 * Geometry invariants a viewer trusts without checking: areas proportional
 * to values, the container exactly covered, no overlaps, deterministic
 * output — and the squarified property itself (no sliver whose aspect
 * ratio the classic algorithm would have avoided).
 */
import { describe, expect, it } from "vitest";
import { squarify } from "../../../studio-ui/src/treemap.js";

const ITEMS = [
  { key: "a", value: 6 },
  { key: "b", value: 6 },
  { key: "c", value: 4 },
  { key: "d", value: 3 },
  { key: "e", value: 2 },
  { key: "f", value: 2 },
  { key: "g", value: 1 },
];

describe("squarify", () => {
  it("areas are proportional and cover the container", () => {
    const rects = squarify(ITEMS, 0, 0, 600, 400);
    const total = ITEMS.reduce((s, i) => s + i.value, 0);
    let covered = 0;
    for (const r of rects) {
      const item = ITEMS.find((i) => i.key === r.key)!;
      const area = r.w * r.h;
      covered += area;
      expect(area / (600 * 400)).toBeCloseTo(item.value / total, 5);
      // Inside the container.
      expect(r.x).toBeGreaterThanOrEqual(-1e-6);
      expect(r.y).toBeGreaterThanOrEqual(-1e-6);
      expect(r.x + r.w).toBeLessThanOrEqual(600 + 1e-6);
      expect(r.y + r.h).toBeLessThanOrEqual(400 + 1e-6);
    }
    expect(covered).toBeCloseTo(600 * 400, 3);
  });

  it("produces no overlapping blocks", () => {
    const rects = squarify(ITEMS, 0, 0, 600, 400);
    for (let i = 0; i < rects.length; i++) {
      for (let j = i + 1; j < rects.length; j++) {
        const a = rects[i];
        const b = rects[j];
        const overlapW = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
        const overlapH = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
        const overlap = Math.max(0, overlapW) * Math.max(0, overlapH);
        expect(overlap, `${a.key} overlaps ${b.key}`).toBeLessThan(1e-6);
      }
    }
  });

  it("keeps blocks square-ish — the property that names the algorithm", () => {
    const rects = squarify(ITEMS, 0, 0, 600, 400);
    for (const r of rects) {
      const aspect = Math.max(r.w / r.h, r.h / r.w);
      // A slice layout would produce ~11:1 for the smallest item here.
      expect(aspect, r.key).toBeLessThan(4);
    }
  });

  it("drops zero-valued items and survives empty input", () => {
    expect(squarify([], 0, 0, 100, 100)).toEqual([]);
    const rects = squarify([{ key: "a", value: 1 }, { key: "z", value: 0 }], 0, 0, 100, 100);
    expect(rects.map((r) => r.key)).toEqual(["a"]);
  });

  it("is deterministic, ties resolved by input order", () => {
    const a = squarify(ITEMS, 0, 0, 600, 400);
    const b = squarify(ITEMS, 0, 0, 600, 400);
    expect(a).toEqual(b);
    expect(a.findIndex((r) => r.key === "a")).toBeLessThan(a.findIndex((r) => r.key === "b"));
  });
});
