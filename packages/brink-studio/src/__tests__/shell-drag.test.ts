/**
 * Strip-icon drag-to-re-dock (shell issue 3.1 / #87, spec §5.1).
 *
 * Covers the extracted pure drag logic: the movement threshold, the
 * pointer-gesture state machine (click vs drag discrimination, click
 * suppression after a completed or cancelled drag), drop-zone hit-testing
 * from rects, and the data-zone → Placement mapping that feeds
 * moveToolWindow. Store re-dock semantics themselves (displacement,
 * open-state carryover) are covered by shell-toolwindows.test.ts (#80).
 */

import { describe, expect, it } from "vitest";
import {
  DRAG_THRESHOLD_PX,
  StripDragGesture,
  createShellLayoutStore,
  exceedsDragThreshold,
  hitTestZone,
  placementFromZone,
  type DockSectionId,
  type ToolWindowDescriptor,
  type ZoneRect,
} from "@brink/studio-shell";

// ── exceedsDragThreshold ────────────────────────────────────────────

describe("exceedsDragThreshold", () => {
  it("is false with no movement", () => {
    expect(exceedsDragThreshold(10, 10, 10, 10)).toBe(false);
  });

  it("is false at exactly the threshold (crossing means strictly beyond)", () => {
    expect(exceedsDragThreshold(0, 0, DRAG_THRESHOLD_PX, 0)).toBe(false);
    expect(exceedsDragThreshold(0, 0, 0, -DRAG_THRESHOLD_PX)).toBe(false);
  });

  it("is true just beyond the threshold, on any axis or diagonal", () => {
    expect(exceedsDragThreshold(0, 0, DRAG_THRESHOLD_PX + 1, 0)).toBe(true);
    expect(exceedsDragThreshold(0, 0, 0, DRAG_THRESHOLD_PX + 1)).toBe(true);
    // (4, 4) is ~5.66px of Euclidean distance — past the 5px default.
    expect(exceedsDragThreshold(0, 0, 4, 4)).toBe(true);
    // (3, 3) is ~4.24px — still a click.
    expect(exceedsDragThreshold(0, 0, 3, 3)).toBe(false);
  });

  it("honors a custom threshold", () => {
    expect(exceedsDragThreshold(0, 0, 8, 0, 10)).toBe(false);
    expect(exceedsDragThreshold(0, 0, 11, 0, 10)).toBe(true);
  });
});

// ── hitTestZone ─────────────────────────────────────────────────────

function zone(
  id: DockSectionId,
  left: number,
  top: number,
  right: number,
  bottom: number,
): ZoneRect {
  return { zone: id, left, top, right, bottom };
}

describe("hitTestZone", () => {
  const zones: ZoneRect[] = [
    zone("left.start", 0, 0, 36, 300),
    zone("left.end", 0, 300, 36, 600),
    zone("bottom.start", 36, 600, 500, 632),
    zone("bottom.end", 500, 600, 964, 632),
  ];

  it("returns the zone containing the point", () => {
    expect(hitTestZone(zones, 18, 150)).toBe("left.start");
    expect(hitTestZone(zones, 18, 450)).toBe("left.end");
    expect(hitTestZone(zones, 600, 616)).toBe("bottom.end");
  });

  it("treats left/top edges as inclusive and right/bottom as exclusive", () => {
    expect(hitTestZone(zones, 0, 0)).toBe("left.start");
    // y = 300 is the boundary: exclusive for left.start, inclusive for left.end.
    expect(hitTestZone(zones, 18, 300)).toBe("left.end");
    expect(hitTestZone(zones, 36, 150)).toBeNull();
    expect(hitTestZone(zones, 18, 600)).toBeNull();
  });

  it("returns null outside every zone, and for an empty list", () => {
    expect(hitTestZone(zones, 400, 300)).toBeNull();
    expect(hitTestZone([], 18, 150)).toBeNull();
  });

  it("is deterministic on overlap: first match wins", () => {
    const overlapping = [zone("right.start", 0, 0, 100, 100), zone("right.end", 0, 0, 100, 100)];
    expect(hitTestZone(overlapping, 50, 50)).toBe("right.start");
  });
});

// ── placementFromZone ───────────────────────────────────────────────

describe("placementFromZone", () => {
  it("maps all six dock-section ids", () => {
    expect(placementFromZone("left.start")).toEqual({ dock: "left", section: "start" });
    expect(placementFromZone("left.end")).toEqual({ dock: "left", section: "end" });
    expect(placementFromZone("right.start")).toEqual({ dock: "right", section: "start" });
    expect(placementFromZone("right.end")).toEqual({ dock: "right", section: "end" });
    expect(placementFromZone("bottom.start")).toEqual({ dock: "bottom", section: "start" });
    expect(placementFromZone("bottom.end")).toEqual({ dock: "bottom", section: "end" });
  });

  it("rejects anything else", () => {
    expect(placementFromZone("")).toBeNull();
    expect(placementFromZone("left")).toBeNull();
    expect(placementFromZone("left.middle")).toBeNull();
    expect(placementFromZone("center.start")).toBeNull();
    expect(placementFromZone("left.start.extra")).toBeNull();
  });
});

// ── StripDragGesture ────────────────────────────────────────────────

describe("StripDragGesture", () => {
  it("a press-and-release without movement is a click, never suppressed", () => {
    const g = new StripDragGesture();
    g.pointerDown(10, 10);
    expect(g.currentPhase).toBe("armed");
    expect(g.pointerUp()).toBe("click");
    expect(g.currentPhase).toBe("idle");
    expect(g.consumeClickSuppression()).toBe(false);
  });

  it("jitter below the threshold stays a click", () => {
    const g = new StripDragGesture();
    g.pointerDown(10, 10);
    expect(g.pointerMove(12, 11)).toBe("ignore");
    expect(g.pointerMove(8, 13)).toBe("ignore");
    expect(g.pointerUp()).toBe("click");
    expect(g.consumeClickSuppression()).toBe(false);
  });

  it("crossing the threshold starts a drag; release is a drop with one suppressed click", () => {
    const g = new StripDragGesture();
    g.pointerDown(10, 10);
    expect(g.pointerMove(30, 10)).toBe("start");
    expect(g.currentPhase).toBe("dragging");
    expect(g.pointerMove(60, 40)).toBe("drag");
    expect(g.pointerUp()).toBe("drop");
    expect(g.currentPhase).toBe("idle");
    // The click that follows the drop is swallowed — exactly once.
    expect(g.consumeClickSuppression()).toBe(true);
    expect(g.consumeClickSuppression()).toBe(false);
  });

  it("the threshold check is relative to the pointerdown position", () => {
    const g = new StripDragGesture();
    g.pointerDown(100, 100);
    expect(g.pointerMove(103, 100)).toBe("ignore");
    expect(g.pointerMove(106, 100)).toBe("start");
  });

  it("cancel mid-drag (Escape) goes idle but still suppresses the trailing click", () => {
    const g = new StripDragGesture();
    g.pointerDown(0, 0);
    expect(g.pointerMove(20, 0)).toBe("start");
    g.cancel();
    expect(g.currentPhase).toBe("idle");
    // The pointer is still down; the eventual release is inert…
    expect(g.pointerMove(40, 0)).toBe("ignore");
    expect(g.pointerUp()).toBe("ignore");
    // …and its click is swallowed.
    expect(g.consumeClickSuppression()).toBe(true);
  });

  it("cancel while merely armed suppresses nothing", () => {
    const g = new StripDragGesture();
    g.pointerDown(0, 0);
    g.cancel();
    expect(g.pointerUp()).toBe("ignore");
    expect(g.consumeClickSuppression()).toBe(false);
  });

  it("events without a preceding pointerdown are ignored", () => {
    const g = new StripDragGesture();
    expect(g.pointerMove(50, 50)).toBe("ignore");
    expect(g.pointerUp()).toBe("ignore");
  });

  it("a new press clears stale, unconsumed suppression", () => {
    const g = new StripDragGesture();
    g.pointerDown(0, 0);
    g.pointerMove(20, 0);
    g.pointerUp(); // drop — suppression set, but no click ever consumed it
    g.pointerDown(0, 0);
    expect(g.pointerUp()).toBe("click");
    expect(g.consumeClickSuppression()).toBe(false);
  });

  it("honors a custom threshold", () => {
    const g = new StripDragGesture(20);
    g.pointerDown(0, 0);
    expect(g.pointerMove(15, 0)).toBe("ignore");
    expect(g.pointerMove(25, 0)).toBe("start");
  });
});

// ── Zone → store hand-off ───────────────────────────────────────────

function desc(id: string): ToolWindowDescriptor {
  return {
    id,
    title: id,
    icon: null,
    defaultPlacement: { dock: "left", section: "start" },
    defaultOpen: true,
    component: () => null,
  };
}

describe("drop hand-off to the layout store", () => {
  it("placementFromZone output drives moveToolWindow", () => {
    const store = createShellLayoutStore();
    store.getState().syncFromRegistry([desc("binder")]);
    expect(store.getState().open["left.start"]).toBe("binder");

    const placement = placementFromZone("bottom.end");
    expect(placement).not.toBeNull();
    if (placement === null) return;
    store.getState().moveToolWindow("binder", placement.dock, placement.section);

    expect(store.getState().placements["binder"]).toEqual({
      dock: "bottom",
      section: "end",
    });
    // It was open, so it opens in the new section (spec §5.1, #80 semantics).
    expect(store.getState().open["left.start"]).toBeNull();
    expect(store.getState().open["bottom.end"]).toBe("binder");
  });

  it("dropping on the section a window already occupies is a no-op", () => {
    const store = createShellLayoutStore();
    store.getState().syncFromRegistry([desc("binder")]);
    const before = store.getState();
    store.getState().moveToolWindow("binder", "left", "start");
    expect(store.getState().placements).toBe(before.placements);
    expect(store.getState().open).toBe(before.open);
  });
});
