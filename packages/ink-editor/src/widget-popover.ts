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
  const panel = document.createElement("div");
  panel.className = "brink-widget-popover";
  panel.setAttribute("role", "dialog");
  document.body.appendChild(panel);
  render(panel);

  let closed = false;
  const close = (): void => {
    if (closed) return;
    closed = true;
    document.removeEventListener("keydown", onKeyDown, true);
    document.removeEventListener("pointerdown", onPointerDown, true);
    window.removeEventListener("resize", reposition);
    window.removeEventListener("scroll", reposition, true);
    panel.remove();
    onClose();
  };

  const reposition = (): void => {
    const a = anchor.getBoundingClientRect();
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
    panel.style.top = `${Math.round(top + window.scrollY)}px`;
    panel.style.left = `${Math.round(left + window.scrollX)}px`;
  };

  const onKeyDown = (e: KeyboardEvent): void => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  };

  const onPointerDown = (e: PointerEvent): void => {
    if (!panel.contains(e.target as Node) && !anchor.contains(e.target as Node)) {
      close();
    }
  };

  reposition();
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
