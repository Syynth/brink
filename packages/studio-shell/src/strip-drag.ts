/**
 * @brink/studio-shell — strip-icon drag-to-re-dock (spec §5.1, issue #87).
 *
 * Dragging a strip icon re-docks its tool window: a ghost chip follows the
 * cursor, the six dock sections render as drop zones on the strips, and the
 * hovered zone highlights; dropping commits the move.
 *
 * Built on pointer events, not HTML5 drag-and-drop: pointer events allow a
 * fully styled ghost, are synthesizable in jsdom and in live verification,
 * and avoid the native DnD quirks (OS drag images, dragover cadence).
 * pointerdown on a strip button *arms* a potential drag and captures the
 * pointer; only crossing a small movement threshold starts it, so a plain
 * click still toggles the window. The click event fired by the pointerup
 * that ends a real drag is suppressed (see StripDragGesture).
 *
 * Two deliberate boundaries:
 * - Dropping calls the layout store's moveToolWindow directly. A re-dock is
 *   a pointer-driven layout-state mutation like a splitter drag, not a
 *   command — clicking remains command-routed (view.toggle.<id>, spec §5.2).
 * - Drag state lives in local React state in the strip/frame layer, not in
 *   the layout store: it is transient interaction state, not layout.
 *
 * Per-pointermove work (ghost position, zone highlight) is imperative DOM
 * via refs, so the frame re-renders only at drag start/end.
 */

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import type { ShellLayoutStore } from "./layout-store.js";
import type { DockSectionId, Placement, ToolWindowDescriptor } from "./toolwindow.js";

/** Movement (px from pointerdown) that turns a press into a drag (§5.1). */
export const DRAG_THRESHOLD_PX = 5;

/** True once the pointer has moved more than `threshold` px from the start. */
export function exceedsDragThreshold(
  startX: number,
  startY: number,
  x: number,
  y: number,
  threshold: number = DRAG_THRESHOLD_PX,
): boolean {
  const dx = x - startX;
  const dy = y - startY;
  return dx * dx + dy * dy > threshold * threshold;
}

/** A drop zone's id plus its viewport rect, for pure hit-testing. */
export interface ZoneRect {
  zone: DockSectionId;
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * The zone containing (x, y), or null. Left/top edges are inclusive,
 * right/bottom exclusive; first match wins (zones never overlap in the
 * shell, so order is irrelevant there — this just keeps it deterministic).
 */
export function hitTestZone(
  zones: readonly ZoneRect[],
  x: number,
  y: number,
): DockSectionId | null {
  for (const z of zones) {
    if (x >= z.left && x < z.right && y >= z.top && y < z.bottom) return z.zone;
  }
  return null;
}

/** Parse a drop zone's data-zone value back into a Placement (null if bogus). */
export function placementFromZone(zone: string): Placement | null {
  switch (zone) {
    case "left.start":
      return { dock: "left", section: "start" };
    case "left.end":
      return { dock: "left", section: "end" };
    case "right.start":
      return { dock: "right", section: "start" };
    case "right.end":
      return { dock: "right", section: "end" };
    case "bottom.start":
      return { dock: "bottom", section: "start" };
    case "bottom.end":
      return { dock: "bottom", section: "end" };
    default:
      return null;
  }
}

export type StripDragPhase = "idle" | "armed" | "dragging";

/**
 * Pure pointer-gesture state machine for a strip button: pointerdown arms,
 * crossing the threshold starts the drag, pointerup is either a click (never
 * crossed) or a drop. DOM-free so the click/drag discrimination and the
 * click-suppression contract are unit-testable.
 *
 * Click suppression: after a gesture that became a real drag — whether it
 * ended in a drop or was cancelled (Escape / pointercancel) — the browser
 * still fires a click on the capturing button. consumeClickSuppression()
 * reports (and clears) that exactly once, so the strip can swallow it; a
 * plain click never sets it. Arming a new gesture clears any stale flag.
 */
export class StripDragGesture {
  private phase: StripDragPhase = "idle";
  private startX = 0;
  private startY = 0;
  private suppressClick = false;
  private readonly threshold: number;

  constructor(threshold: number = DRAG_THRESHOLD_PX) {
    this.threshold = threshold;
  }

  get currentPhase(): StripDragPhase {
    return this.phase;
  }

  /** Arm a potential drag at the pointerdown position. */
  pointerDown(x: number, y: number): void {
    this.phase = "armed";
    this.startX = x;
    this.startY = y;
    // A new press invalidates any unconsumed suppression (e.g. a drop whose
    // click event never arrived) so it cannot swallow an unrelated click.
    this.suppressClick = false;
  }

  /**
   * "start" on the move that first crosses the threshold, "drag" while
   * dragging, "ignore" otherwise (idle, or armed but below threshold).
   */
  pointerMove(x: number, y: number): "ignore" | "start" | "drag" {
    if (this.phase === "dragging") return "drag";
    if (this.phase !== "armed") return "ignore";
    if (!exceedsDragThreshold(this.startX, this.startY, x, y, this.threshold)) {
      return "ignore";
    }
    this.phase = "dragging";
    this.suppressClick = true;
    return "start";
  }

  /** "click" if the press never became a drag, "drop" if it did. */
  pointerUp(): "ignore" | "click" | "drop" {
    if (this.phase === "armed") {
      this.phase = "idle";
      return "click";
    }
    if (this.phase === "dragging") {
      this.phase = "idle";
      return "drop";
    }
    return "ignore";
  }

  /** Abort (Escape, pointercancel). Suppression survives if a drag started. */
  cancel(): void {
    this.phase = "idle";
  }

  /** True exactly once after a real drag — swallow the click that follows. */
  consumeClickSuppression(): boolean {
    const suppress = this.suppressClick;
    this.suppressClick = false;
    return suppress;
  }
}

// ── React glue ──────────────────────────────────────────────────────

/** The tool window under drag, as the ghost chip renders it. */
export interface StripDragState {
  id: string;
  title: string;
  icon: ReactNode;
}

/** Pointer handlers for one strip button (spread onto the <button>). */
export interface StripDragHandlers {
  onPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerMove: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerUp: (event: ReactPointerEvent<HTMLButtonElement>) => void;
  onPointerCancel: (event: ReactPointerEvent<HTMLButtonElement>) => void;
}

export interface StripDragController {
  /** The window being dragged, or null. Strips render drop zones when set. */
  dragging: StripDragState | null;
  /** Callback ref for the ghost chip — positions it at the pointer on mount. */
  setGhostElement: (element: HTMLDivElement | null) => void;
  /** Drag handlers for one strip button. */
  handlersFor: (descriptor: ToolWindowDescriptor) => StripDragHandlers;
  /** For the strip's onClick: true means this click ended a drag — skip it. */
  consumeClickSuppression: () => boolean;
}

const DROPZONE_SELECTOR = ".shell-strip-dropzone[data-zone]";
const GHOST_OFFSET_X = 12;
const GHOST_OFFSET_Y = 14;

function positionGhost(element: HTMLElement, point: { x: number; y: number }): void {
  element.style.transform = `translate(${point.x + GHOST_OFFSET_X}px, ${point.y + GHOST_OFFSET_Y}px)`;
}

/**
 * Drag-to-re-dock controller for the shell's strips. One instance per
 * ShellFrame; all strip buttons share it (a single pointer drags at a time).
 *
 * Hit-testing is rect math over the rendered drop zones (queried per move —
 * six small elements), not document.elementFromPoint: rects are deterministic,
 * unaffected by the ghost or pointer capture, and the pure hitTestZone core
 * is unit-tested.
 */
export function useStripDrag(layout: ShellLayoutStore): StripDragController {
  const [dragging, setDragging] = useState<StripDragState | null>(null);
  const [gesture] = useState(() => new StripDragGesture());
  // Per-move bookkeeping is refs, not state: pointermove must not re-render.
  const dragIdRef = useRef<string | null>(null);
  const ghostRef = useRef<HTMLDivElement | null>(null);
  const lastPointRef = useRef({ x: 0, y: 0 });
  const hoverRef = useRef<{ zone: DockSectionId; element: HTMLElement } | null>(null);

  const finishDrag = useCallback(
    (commit: boolean): void => {
      const id = dragIdRef.current;
      const hover = hoverRef.current;
      if (commit && id !== null && hover !== null) {
        const placement = placementFromZone(hover.zone);
        if (placement !== null) {
          // Direct store mutation by design — see module doc comment.
          layout.getState().moveToolWindow(id, placement.dock, placement.section);
        }
      }
      hover?.element.classList.remove("active");
      hoverRef.current = null;
      dragIdRef.current = null;
      document.body.style.removeProperty("cursor");
      setDragging(null);
    },
    [layout],
  );

  // Escape cancels an active drag (capture-phase, mirroring maximize-restore
  // in ShellFrame; preventDefault keeps other Escape handlers out).
  useEffect(() => {
    if (dragging === null) return undefined;
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      gesture.cancel();
      finishDrag(false);
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [dragging, gesture, finishDrag]);

  const setGhostElement = useCallback((element: HTMLDivElement | null): void => {
    ghostRef.current = element;
    if (element !== null) positionGhost(element, lastPointRef.current);
  }, []);

  /** Re-resolve the hovered drop zone from the pointer position. */
  const updateHover = useCallback((x: number, y: number): void => {
    const zones: ZoneRect[] = [];
    const elements = new Map<DockSectionId, HTMLElement>();
    document.querySelectorAll<HTMLElement>(DROPZONE_SELECTOR).forEach((element) => {
      const zone = element.getAttribute("data-zone") ?? "";
      if (placementFromZone(zone) === null) return;
      const rect = element.getBoundingClientRect();
      zones.push({
        zone: zone as DockSectionId,
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
      });
      elements.set(zone as DockSectionId, element);
    });
    const hit = hitTestZone(zones, x, y);
    const previous = hoverRef.current;
    if (previous !== null && previous.zone === hit) return;
    previous?.element.classList.remove("active");
    const element = hit !== null ? elements.get(hit) : undefined;
    if (hit !== null && element !== undefined) {
      element.classList.add("active");
      hoverRef.current = { zone: hit, element };
    } else {
      hoverRef.current = null;
    }
  }, []);

  const handlersFor = useCallback(
    (descriptor: ToolWindowDescriptor): StripDragHandlers => ({
      onPointerDown: (event) => {
        if (event.button !== 0 || !event.isPrimary) return;
        gesture.pointerDown(event.clientX, event.clientY);
        // Capture immediately so the threshold check and the whole drag keep
        // receiving moves even when the cursor leaves this 28px button.
        // (jsdom has no pointer-capture implementation — guard.)
        if (typeof event.currentTarget.setPointerCapture === "function") {
          event.currentTarget.setPointerCapture(event.pointerId);
        }
      },
      onPointerMove: (event) => {
        const result = gesture.pointerMove(event.clientX, event.clientY);
        if (result === "ignore") return;
        lastPointRef.current = { x: event.clientX, y: event.clientY };
        if (result === "start") {
          dragIdRef.current = descriptor.id;
          document.body.style.cursor = "grabbing";
          setDragging({
            id: descriptor.id,
            title: descriptor.title,
            icon: descriptor.icon,
          });
        }
        if (ghostRef.current !== null) {
          positionGhost(ghostRef.current, lastPointRef.current);
        }
        // On "start" the zones haven't rendered yet — this resolves to null,
        // and the next move highlights normally.
        updateHover(event.clientX, event.clientY);
      },
      onPointerUp: () => {
        if (gesture.pointerUp() === "drop") finishDrag(true);
      },
      onPointerCancel: () => {
        gesture.cancel();
        if (dragIdRef.current !== null) finishDrag(false);
      },
    }),
    [gesture, finishDrag, updateHover],
  );

  const consumeClickSuppression = useCallback(
    () => gesture.consumeClickSuppression(),
    [gesture],
  );

  return { dragging, setGhostElement, handlersFor, consumeClickSuppression };
}
