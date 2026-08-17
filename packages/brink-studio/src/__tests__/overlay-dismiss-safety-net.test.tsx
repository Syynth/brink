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

    // Overlay's mount installs two document-level capture-phase keydown
    // listeners in sequence: its own local dismiss listener (the first
    // effect, added directly) and the registry's global safety-net listener
    // (the second effect, installed lazily by `registerDismissible` —
    // guaranteed fresh here by the reset above).
    const keydownCalls = addSpy.mock.calls.filter(
      (call) => call[0] === "keydown" && call[2] === true,
    );
    expect(keydownCalls).toHaveLength(2);
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
});
