/**
 * Studio-owned modal chrome for argument-widget editors (argument-widget-spec
 * §1 / §6.4). A full-overlay alternative to the popover, for heavy host editors
 * (the map-editor case) declared with `surface: "modal"`. A backdrop centers the
 * panel; Escape or a backdrop click dismisses. The widget fills the panel; the
 * chrome owns placement + dismissal.
 */

export interface ModalHandle {
  close(): void;
}

/** Open a modal, rendering into the centered panel. `onClose` fires once on any
 *  dismissal (Escape, backdrop click, or `close()`).
 *
 *  `mount` is where the backdrop is appended — pass the `.brink-studio` root so
 *  the panel and any host content inherit the theme's `--bs-*` tokens (they are
 *  scoped to that root). The backdrop is `position: fixed`, so it still covers
 *  the viewport regardless of where it is parented. Defaults to `document.body`. */
export function openModal(
  render: (container: HTMLElement) => void,
  onClose: () => void,
  mount: HTMLElement = document.body,
): ModalHandle {
  const backdrop = document.createElement("div");
  backdrop.className = "brink-widget-modal-backdrop";
  const panel = document.createElement("div");
  panel.className = "brink-widget-modal";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-modal", "true");
  backdrop.appendChild(panel);
  mount.appendChild(backdrop);
  render(panel);

  let closed = false;
  const close = (): void => {
    if (closed) return;
    closed = true;
    document.removeEventListener("keydown", onKeyDown, true);
    backdrop.remove();
    onClose();
  };

  const onKeyDown = (e: KeyboardEvent): void => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
  };

  backdrop.addEventListener("pointerdown", (e) => {
    if (e.target === backdrop) close();
  });
  // Defer so the click that opened the modal doesn't immediately dismiss it.
  setTimeout(() => {
    if (!closed) document.addEventListener("keydown", onKeyDown, true);
  }, 0);

  return { close };
}
