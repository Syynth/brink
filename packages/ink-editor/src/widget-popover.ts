/**
 * Studio-owned popover chrome for argument-widget editors (argument-widget-spec
 * §1 — "the studio owns the chrome"). A lightweight DOM popover anchored to a
 * CodeMirror inline widget: positioned beneath the anchor, dismissed on Escape
 * or click-outside. The widget fills `container`; the chrome owns placement,
 * dismissal, and focus.
 *
 * DOM-level (not the React shell overlay) because the anchor is a CodeMirror
 * widget living in plain editor DOM; bridging React contexts from there is more
 * machinery than the popover warrants. "Studio owns the chrome" still holds —
 * it is studio code with consistent styling.
 */

import { ensureStructuralStyles } from "./structural-styles.js";
import { registerDismissible } from "./dismiss-registry.js";

export interface PopoverHandle {
  close(): void;
}

/**
 * Open a popover anchored to `anchor`, rendering into the provided container.
 * `onClose` fires once, on any dismissal (Escape, click-outside, or `close()`).
 */
export function openPopover(
  anchor: HTMLElement,
  render: (container: HTMLElement) => void,
  onClose: () => void,
): PopoverHandle {
  // Capture the anchor rect BEFORE rendering. A widget editor may edit the doc
  // on first interaction, and for Fill the ghost anchor is replaced by a swatch
  // the instant a literal is inserted — detaching it. Fall back to the captured
  // rect so the popover stays put instead of jumping to (0, 0).
  let anchorRect = anchor.getBoundingClientRect();

  ensureStructuralStyles();
  const panel = document.createElement("div");
  panel.className = "brink-widget-popover";
  panel.setAttribute("role", "dialog");
  // Mount inside the studio root so the panel and any embedded host content
  // inherit the theme's `--bs-*` tokens (scoped to `.brink-studio`). The panel
  // is `position: fixed`, so it stays viewport-anchored regardless of parent.
  const mount = anchor.closest<HTMLElement>(".brink-studio") ?? document.body;
  mount.appendChild(panel);
  render(panel);

  let closed = false;
  // Reposition when the panel's own size changes — e.g. the form's inline color
  // picker expanding — so a grown panel re-flips instead of clipping off-screen.
  const resizeObserver = new ResizeObserver(() => reposition());
  // Global Escape safety net (#279) — a second, independent registration
  // alongside the popover's own listeners below (see dismiss-registry.ts).
  const unregisterDismiss = registerDismissible(() => close());

  const close = (): void => {
    if (closed) return;
    closed = true;
    document.removeEventListener("keydown", onKeyDown, true);
    document.removeEventListener("pointerdown", onPointerDown, true);
    window.removeEventListener("resize", reposition);
    window.removeEventListener("scroll", reposition, true);
    resizeObserver.disconnect();
    unregisterDismiss();
    panel.remove();
    onClose();
  };

  const reposition = (): void => {
    if (anchor.isConnected) anchorRect = anchor.getBoundingClientRect();
    const a = anchorRect;
    const p = panel.getBoundingClientRect();
    const margin = 4;
    // Prefer below-left-aligned; flip above when it would overflow the viewport.
    let top = a.bottom + margin;
    if (top + p.height > window.innerHeight && a.top - margin - p.height >= 0) {
      top = a.top - margin - p.height;
    }
    let left = a.left;
    if (left + p.width > window.innerWidth - margin) {
      left = Math.max(margin, window.innerWidth - margin - p.width);
    }
    // Fixed positioning is viewport-relative — no scroll offset. Placement is
    // data (custom properties consumed by the `.brink-widget-popover` class
    // rule), keeping the panel free of inline styles for host restyling (#363).
    panel.style.setProperty("--brink-popup-top", `${Math.round(top)}px`);
    panel.style.setProperty("--brink-popup-left", `${Math.round(left)}px`);
  };

  const onKeyDown = (e: KeyboardEvent): void => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  };

  const onPointerDown = (e: PointerEvent): void => {
    // Only the panel is consulted — the anchor may have been replaced (Fill).
    if (!panel.contains(e.target as Node)) {
      close();
    }
  };

  reposition();
  resizeObserver.observe(panel);
  // Defer listener attachment so the click that opened the popover doesn't
  // immediately dismiss it.
  setTimeout(() => {
    if (closed) return;
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("resize", reposition);
    window.addEventListener("scroll", reposition, true);
  }, 0);

  return { close };
}
