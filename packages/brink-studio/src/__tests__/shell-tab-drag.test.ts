/**
 * Editor tab drag (issue #142, spec §7.8) — the pure logic under
 * tab-drag.ts: insertion-gap math over tab rects and the gap → final-index
 * conversion for within-group reorders. The gesture machine itself
 * (StripDragGesture: threshold, click suppression) is shared with strip
 * drag and covered by shell-drag.test.ts; the full pointer flow is e2e
 * (tab-drag.spec.ts).
 */

import { describe, expect, it } from "vitest";
import { insertionIndexForX, reorderTargetIndex, type TabRect } from "@brink/studio-shell";

/** Three 120px tabs: [0,120), [120,240), [240,360). */
const TABS: TabRect[] = [
  { left: 0, right: 120 },
  { left: 120, right: 240 },
  { left: 240, right: 360 },
];

describe("insertionIndexForX", () => {
  it("returns the gap before the first tab whose midpoint is right of x", () => {
    expect(insertionIndexForX(TABS, 0)).toBe(0);
    expect(insertionIndexForX(TABS, 59)).toBe(0); // left half of tab 0
    expect(insertionIndexForX(TABS, 61)).toBe(1); // right half of tab 0
    expect(insertionIndexForX(TABS, 179)).toBe(1);
    expect(insertionIndexForX(TABS, 181)).toBe(2);
  });

  it("returns the tail gap past the last midpoint (append)", () => {
    expect(insertionIndexForX(TABS, 301)).toBe(3);
    expect(insertionIndexForX(TABS, 1000)).toBe(3); // empty bar tail
  });

  it("midpoints themselves fall to the right gap (x < mid is strict)", () => {
    expect(insertionIndexForX(TABS, 60)).toBe(1);
  });

  it("an empty bar always appends at 0", () => {
    expect(insertionIndexForX([], 50)).toBe(0);
  });
});

describe("reorderTargetIndex", () => {
  it("gaps at or before the tab's own slot are the final index unchanged", () => {
    expect(reorderTargetIndex(2, 0)).toBe(0);
    expect(reorderTargetIndex(2, 1)).toBe(1);
    expect(reorderTargetIndex(2, 2)).toBe(2); // own left gap — no-op
  });

  it("gaps after the tab's slot shift down by one (the tab vacates first)", () => {
    expect(reorderTargetIndex(0, 1)).toBe(0); // own right gap — no-op
    expect(reorderTargetIndex(0, 2)).toBe(1);
    expect(reorderTargetIndex(0, 3)).toBe(2); // tail of a 3-tab bar
  });
});
