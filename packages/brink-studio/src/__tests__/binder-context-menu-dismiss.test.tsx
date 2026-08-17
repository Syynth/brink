/**
 * `BinderContextMenu`'s dismiss listeners (#279 audit finding).
 *
 * Before this fix the menu's outside-pointerdown/Escape listeners were
 * BUBBLE-phase (`document.addEventListener("mousedown"/"keydown", handler)`
 * — no capture flag), unlike `Overlay`'s capture-phase contract
 * (overlay.tsx). Any element BETWEEN the event's target and `document` that
 * called `stopPropagation()` during the bubble phase silently defeated BOTH
 * Escape and outside-dismiss at once, since they shared that one un-
 * capturing pair of listeners — exactly the shape of #279's stuck menu:
 * "Escape did nothing and clicking elsewhere did not dismiss it."
 *
 * `BinderContextMenu` is the single most-reused menu in the app: rendered
 * directly by `Binder.tsx` (binder right-click, #279's own first suspect)
 * and again by `SymbolContextMenuHost` for the editor/Story-Graph symbol
 * menu. Fixing it once here covers both.
 *
 * This proves the fix: capture-phase document listeners run BEFORE any
 * bubble-phase `stopPropagation()` gets a chance to run at all, so an
 * unrelated ancestor stopping propagation can no longer defeat dismissal.
 */

import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { BinderContextMenu } from "@brink/studio-ui";
import { resetDismissRegistryForTests } from "@brink/studio-shell";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  resetDismissRegistryForTests();
});

function mountMenu(onClose: () => void): void {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(BinderContextMenu, {
        x: 10,
        y: 10,
        target: { kind: "file", path: "main.ink", canDelete: true, canRename: true },
        outline: [],
        onAction: () => {},
        onClose,
      }),
    );
  });
}

describe("BinderContextMenu dismissal survives a bubble-phase stopPropagation() (#279)", () => {
  it("closes on outside pointerdown even when an unrelated ancestor stops bubble propagation", () => {
    resetDismissRegistryForTests();
    let closed = false;
    // An unrelated element elsewhere in the app that stops bubble
    // propagation on pointerdown — a drag handler, a focus trap, anything
    // that isn't the menu and isn't trying to defeat it.
    const blocker = document.createElement("div");
    blocker.addEventListener("pointerdown", (e) => e.stopPropagation());
    document.body.appendChild(blocker);

    mountMenu(() => {
      closed = true;
    });

    blocker.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
    expect(closed).toBe(true);

    blocker.remove();
  });

  it("closes on Escape even when an unrelated ancestor stops bubble propagation", () => {
    resetDismissRegistryForTests();
    let closed = false;
    const blocker = document.createElement("div");
    blocker.addEventListener("keydown", (e) => e.stopPropagation());
    document.body.appendChild(blocker);

    mountMenu(() => {
      closed = true;
    });

    blocker.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(closed).toBe(true);

    blocker.remove();
  });

  it("does NOT close on a pointerdown inside the menu itself", () => {
    resetDismissRegistryForTests();
    let closed = false;
    mountMenu(() => {
      closed = true;
    });

    const item = container!.querySelector(".brink-context-menu-item");
    expect(item).not.toBeNull();
    item!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true }));
    expect(closed).toBe(false);
  });

  it("on the SECOND open, with the net already pre-installed, Escape still runs the menu's own close (preventDefault()) — not just the net's bare dismiss", () => {
    // No reset before the first open — recreates production: the net
    // installs once (on this first `registerDismissible()` call) and stays
    // installed across the second menu open. Before the ordering fix, the
    // net's `document`-capture listener would run BEFORE a freshly-mounted
    // second menu's own `document`-capture listener (registered later),
    // handling the Escape itself and never giving this menu's own listener a
    // chance to run its `preventDefault()`.
    resetDismissRegistryForTests();

    let firstClosed = false;
    mountMenu(() => {
      firstClosed = true;
    });
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(firstClosed).toBe(true);
    act(() => root?.unmount());
    container?.remove();
    root = null;
    container = null;

    let secondClosed = false;
    mountMenu(() => {
      secondClosed = true;
    });
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    document.dispatchEvent(event);
    expect(secondClosed).toBe(true);
    // The menu's own capture-phase listener handled it, not a bare fallback
    // from the net — proven by preventDefault() having run.
    expect(event.defaultPrevented).toBe(true);
  });
});
