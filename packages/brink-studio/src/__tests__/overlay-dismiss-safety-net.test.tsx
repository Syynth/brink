/**
 * The global "dismiss all transient surfaces on Escape" safety net (#279
 * part b, dismiss-registry.ts) — proven against a REAL `Overlay`, the
 * primitive every studio-shell menu/popover/palette already routes through.
 *
 * `Overlay` itself already dismisses correctly on Escape / outside-
 * pointerdown through its own per-instance listeners (overlay.tsx) — that
 * was true before #279 and this file does not re-prove it. What #279 asks
 * for is resilience against the ORPHAN case it names explicitly: a surface
 * whose own dismiss listener is lost (a re-render/error detaching it) while
 * it stays mounted and visibly open. A test that presses Escape and lets
 * Overlay's own listener catch it proves nothing about that case — its own
 * listener is exactly what's supposed to be missing.
 *
 * This test forces the orphan directly: it captures the exact capture-phase
 * `keydown` listener Overlay's own dismiss effect installed, removes JUST
 * that one from `document` (simulating the detachment #279 describes)
 * without touching Overlay's SEPARATE registry-registration effect, then
 * proves Escape still closes it — because that second effect is
 * independent of the first, not derived from it.
 */

import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Overlay, resetDismissRegistryForTests } from "@brink/studio-shell";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let container: HTMLDivElement | null = null;
let anchor: HTMLButtonElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  anchor?.remove();
  root = null;
  container = null;
  anchor = null;
  resetDismissRegistryForTests();
});

function mountOverlay(onClose: () => void): void {
  anchor = document.createElement("button");
  document.body.appendChild(anchor);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(Overlay, {
        open: true,
        onClose,
        anchor,
        children: createElement("div", null, "menu content"),
      }),
    );
  });
}

describe("Overlay Escape safety net — orphaned local listener (#279)", () => {
  it("still closes on Escape when the overlay's own local listener has been detached while it stays mounted", () => {
    resetDismissRegistryForTests();
    const addSpy = vi.spyOn(document, "addEventListener");
    const onClose = vi.fn();

    mountOverlay(onClose);

    // Overlay's mount installs exactly ONE document-level capture-phase
    // keydown listener: its own local dismiss listener. The registry's
    // global safety-net listener is installed on `window`, bubble phase —
    // deliberately NOT on `document`/capture, so it always runs AFTER
    // (never instead of) a surface's own capture-phase handler; see the
    // "LISTENER ORDERING" note on dismiss-registry.ts.
    const keydownCalls = addSpy.mock.calls.filter(
      (call) => call[0] === "keydown" && call[2] === true,
    );
    expect(keydownCalls).toHaveLength(1);
    const localHandler = keydownCalls[0][1] as EventListener;
    addSpy.mockRestore();

    // Simulate the orphan #279 describes: Overlay's own dismiss listener is
    // gone — WITHOUT going through Overlay's effect cleanup, which would
    // also unregister from the safety net. The whole point of the fix is
    // that this specific failure mode leaves the registry entry intact even
    // though the local listener is gone.
    document.removeEventListener("keydown", localHandler, true);

    expect(onClose).not.toHaveBeenCalled();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    // The now-detached local listener could not have fired this — only the
    // independent global safety-net registration (a separate effect from
    // the one whose listener was just removed) could have.
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("a normal Escape press (nothing orphaned) closes exactly once, not twice", () => {
    resetDismissRegistryForTests();
    const onClose = vi.fn();
    mountOverlay(onClose);

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    // Overlay's own listener handles it and calls preventDefault(), so the
    // global net's `event.defaultPrevented` guard skips a redundant second
    // call — the safety net stays inert unless it's actually needed.
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("still works correctly on the SECOND open, with the net already pre-installed (production shape)", () => {
    // Deliberately no `resetDismissRegistryForTests()` here. In production
    // `installGlobalDismissNet()` runs once, the first time ANY surface in
    // the whole app registers, and stays installed for the process's
    // lifetime — every subsequent surface's own listener is what gets
    // (re)attached late, after the net. A test that resets before every
    // mount inverts that: it forces the net to install AFTER the surface's
    // own listener every time, which is the one arrangement that could never
    // expose an ordering bug. This test intentionally recreates the
    // production order: net installed once up front (via the first overlay
    // below), still installed for the second.
    const first = vi.fn();
    mountOverlay(first);
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(first).toHaveBeenCalledTimes(1);
    act(() => root?.unmount());
    container?.remove();
    anchor?.remove();

    // Second open: the net's `window` bubble-phase listener has been live
    // since the FIRST mount above; this overlay's own `document` capture
    // listener is attached fresh, after it. If the net ran first (the pre-fix
    // `document`-capture shape), it would call `onClose` before this
    // overlay's own listener got to call `preventDefault()` — stranding
    // `event.defaultPrevented` at `false` and leaving `onClose` invoked, but
    // for the wrong reason and with no guarantee only the top-most surface
    // closed.
    const onClose = vi.fn();
    mountOverlay(onClose);
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    document.dispatchEvent(event);
    // The overlay's own capture-phase listener handled it (and called
    // preventDefault()) — not a bare fallback from the net.
    expect(event.defaultPrevented).toBe(true);
    // Closed exactly once: the net's defaultPrevented guard kept it from
    // ALSO firing.
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("a single Escape does not sweep every registered surface when one already handled it", () => {
    // The pre-fix bug (finding 1c): because `dismissAllTransientSurfaces()`
    // closes EVERY registered surface unconditionally, a `document`-capture
    // net that ran before any surface's own listener turned one Escape press
    // into "close the entire overlay stack" instead of "close the surface
    // that actually handled it". With the net on `window`/bubble, it never
    // gets a turn at all once some surface's own capture-phase listener has
    // already called `preventDefault()` — so only that one surface closes.
    resetDismissRegistryForTests();
    const closedA: boolean[] = [];
    const closedB: boolean[] = [];
    const onCloseA = () => closedA.push(true);
    const onCloseB = () => closedB.push(true);

    // Two independent Overlay instances registered simultaneously — modeling
    // a stacked-surface scenario (e.g. a popover opened from within a menu).
    const anchorA = document.createElement("button");
    const anchorB = document.createElement("button");
    document.body.append(anchorA, anchorB);
    const containerA = document.createElement("div");
    const containerB = document.createElement("div");
    document.body.append(containerA, containerB);
    const rootA = createRoot(containerA);
    const rootB = createRoot(containerB);
    act(() => {
      rootA.render(
        createElement(Overlay, {
          open: true,
          onClose: onCloseA,
          anchor: anchorA,
          children: createElement("div", null, "menu A"),
        }),
      );
      rootB.render(
        createElement(Overlay, {
          open: true,
          onClose: onCloseB,
          anchor: anchorB,
          children: createElement("div", null, "menu B"),
        }),
      );
    });

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );

    // Exactly one of the two closed — never both from a single Escape once
    // either one's own listener has claimed the event via preventDefault().
    const totalClosed = closedA.length + closedB.length;
    expect(totalClosed).toBe(1);

    act(() => {
      rootA.unmount();
      rootB.unmount();
    });
    containerA.remove();
    containerB.remove();
    anchorA.remove();
    anchorB.remove();
  });
});
