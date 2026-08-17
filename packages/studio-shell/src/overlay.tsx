/**
 * @brink/studio-shell — overlay primitive (docs/studio-shell-spec.md §7.7).
 *
 * The one anchored/floating surface under every transient UI: command palette,
 * context menus, dropdowns, popovers, tooltips. Owns anchored positioning with
 * viewport-edge flipping (floating-ui), dismiss on outside-pointerdown/Escape,
 * and focus return to the previously focused element.
 *
 * Two modes: pass `anchor` for an anchored popover; omit it for a centered
 * top surface (palette-style). Rendered in place (inside the .brink-studio
 * scope so design tokens apply), not portaled.
 */

import { useEffect, useLayoutEffect, useRef, type ReactNode } from "react";
import {
  autoUpdate,
  flip,
  offset,
  shift,
  useFloating,
  type Placement,
} from "@floating-ui/react-dom";
import { registerDismissible } from "./dismiss-registry.js";

export interface OverlayProps {
  open: boolean;
  onClose(): void;
  /** Anchored popover when set; centered top surface when omitted. */
  anchor?: HTMLElement | null;
  placement?: Placement;
  className?: string;
  children: ReactNode;
}

export function Overlay({
  open,
  onClose,
  anchor,
  placement = "bottom-start",
  className,
  children,
}: OverlayProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  const anchored = anchor !== undefined && anchor !== null;
  const { refs, floatingStyles } = useFloating({
    placement,
    open,
    whileElementsMounted: autoUpdate,
    middleware: [offset(4), flip({ padding: 8 }), shift({ padding: 8 })],
  });

  useLayoutEffect(() => {
    if (anchored) refs.setReference(anchor);
  }, [anchored, anchor, refs]);

  // Focus return: remember what was focused when we opened; restore on close.
  const previousFocus = useRef<Element | null>(null);
  useEffect(() => {
    if (!open) return;
    previousFocus.current = document.activeElement;
    return () => {
      const prev = previousFocus.current;
      if (prev instanceof HTMLElement && prev.isConnected) {
        prev.focus({ preventScroll: true });
      }
    };
  }, [open]);

  // Dismissal: Escape anywhere, pointerdown outside the surface (and outside
  // the anchor, so an anchor acting as a toggle button doesn't double-fire).
  useEffect(() => {
    if (!open) return;

    const onPointerDown = (event: PointerEvent): void => {
      const container = containerRef.current;
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (container?.contains(target)) return;
      if (anchored && anchor.contains(target)) return;
      onClose();
    };
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      onClose();
    };

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open, onClose, anchored, anchor]);

  // Global Escape safety net (#279) — registered in a SEPARATE, minimal
  // effect from the dismiss-listener effect above, so a bug in that logic
  // (or a re-render that orphans it while this overlay stays mounted) can't
  // take this registration down with it. See dismiss-registry.ts.
  useEffect(() => {
    if (!open) return;
    return registerDismissible(onClose);
  }, [open, onClose]);

  if (!open) return null;

  const classes = ["shell-overlay", anchored ? "anchored" : "centered", className]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      ref={(node) => {
        containerRef.current = node;
        if (anchored) refs.setFloating(node);
      }}
      className={classes}
      style={anchored ? floatingStyles : undefined}
      role="dialog"
    >
      {children}
    </div>
  );
}
