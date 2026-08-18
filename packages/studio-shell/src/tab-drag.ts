/**
 * @brink/studio-shell — editor tab drag (spec §7.8, issue #142).
 *
 * Dragging a tab reorders it within its group or moves it to another group:
 * a tab-shaped ghost follows the cursor, the hovered tab bar shows an insert
 * indicator at the gap under the pointer, and dropping commits through the
 * editor-groups store (`reorderTab` within a group, `moveTabToGroup` with an
 * insertion index across groups).
 *
 * This is the strip-drag pattern (§5.1) applied to tabs — it reuses the same
 * DOM-free pointer-gesture machine (StripDragGesture: pointerdown arms,
 * 5px threshold starts, click suppression after a real drag) rather than
 * forking it. The differences are the drop model (insertion gaps along a
 * tab bar instead of six fixed zones) and the commit (groups store, not the
 * layout store), so this lives as a sibling controller, not a generalization
 * of useStripDrag.
 *
 * Same boundaries as strip-drag: dropping mutates the groups store directly
 * (a pointer-driven structural edit, like a splitter drag — clicking stays
 * command-free tab activation); drag state is transient local React state;
 * per-pointermove work (ghost position, insert indicator) is imperative DOM
 * via refs so the editor area re-renders only at drag start/end.
 */

import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { documentKey } from "./document.js";
import type { EditorGroupsStore, EditorTab } from "./editor-groups.js";
import { StripDragGesture } from "./strip-drag.js";

/** A tab's horizontal extent, for pure insertion-gap math. */
export interface TabRect {
  left: number;
  right: number;
}

/**
 * The insertion gap (0..tabs.length) for a pointer at `x` over a tab bar:
 * before the first tab whose midpoint lies right of the pointer, else the
 * tail (append). Pure — unit-testable without DOM.
 */
export function insertionIndexForX(tabs: readonly TabRect[], x: number): number {
  for (let i = 0; i < tabs.length; i++) {
    if (x < (tabs[i].left + tabs[i].right) / 2) return i;
  }
  return tabs.length;
}

/**
 * Convert an insertion gap into the dragged tab's final index for a
 * within-group reorder: gaps after the tab's own slot shift down by one
 * because the tab vacates its position first. (Gap fromIndex and
 * fromIndex + 1 are both the no-op drop on the tab's own position.)
 */
export function reorderTargetIndex(fromIndex: number, gapIndex: number): number {
  return gapIndex > fromIndex ? gapIndex - 1 : gapIndex;
}

// ── React glue ──────────────────────────────────────────────────────

/** The tab under drag, as the ghost renders it. */
export interface TabDragState {
  key: string;
  title: string;
  fromGroupId: string;
}

/** Pointer handlers for one tab (spread onto the tab element). */
export interface TabDragHandlers {
  onPointerDown: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerMove: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerUp: (event: ReactPointerEvent<HTMLDivElement>) => void;
  onPointerCancel: (event: ReactPointerEvent<HTMLDivElement>) => void;
}

export interface TabDragController {
  /** The tab being dragged, or null. The editor area renders the ghost when set. */
  dragging: TabDragState | null;
  /** Callback ref for the ghost — positions it at the pointer on mount. */
  setGhostElement: (element: HTMLDivElement | null) => void;
  /** Drag handlers for one tab. */
  handlersFor: (groupId: string, tab: EditorTab) => TabDragHandlers;
  /** For the tab's onClick: true means this click ended a drag — skip it. */
  consumeClickSuppression: () => boolean;
}

const TAB_BAR_SELECTOR = ".brink-file-tabs[data-group]";
const TAB_SELECTOR = ".brink-tab";
const DROP_BEFORE_CLASS = "brink-tab-drop-before";
const DROP_AFTER_CLASS = "brink-tab-drop-after";
const DROP_EMPTY_CLASS = "brink-file-tabs-drop-empty";
const GHOST_OFFSET_X = 10;
const GHOST_OFFSET_Y = 12;

interface HoverTarget {
  groupId: string;
  /** Insertion gap in the bar's tab list (0..tabs.length). */
  index: number;
  /** The element carrying the indicator class, for cleanup. */
  element: HTMLElement;
  className: string;
}

function positionGhost(element: HTMLElement, point: { x: number; y: number }): void {
  element.style.transform = `translate(${point.x + GHOST_OFFSET_X}px, ${point.y + GHOST_OFFSET_Y}px)`;
}

/**
 * Tab-drag controller for the editor area. One instance per EditorArea; all
 * group tab bars share it (a single pointer drags at a time).
 *
 * Hit-testing is rect math over the rendered tab bars (queried per move),
 * mirroring strip-drag: bar rects decide which group is hovered, tab rects
 * feed the pure insertionIndexForX gap math.
 */
export function useTabDrag(groups: EditorGroupsStore): TabDragController {
  const [dragging, setDragging] = useState<TabDragState | null>(null);
  const [gesture] = useState(() => new StripDragGesture());
  // Per-move bookkeeping is refs, not state: pointermove must not re-render.
  const dragRef = useRef<TabDragState | null>(null);
  const ghostRef = useRef<HTMLDivElement | null>(null);
  const lastPointRef = useRef({ x: 0, y: 0 });
  const hoverRef = useRef<HoverTarget | null>(null);

  const finishDrag = useCallback(
    (commit: boolean): void => {
      const drag = dragRef.current;
      const hover = hoverRef.current;
      if (commit && drag !== null && hover !== null) {
        const state = groups.getState();
        if (hover.groupId === drag.fromGroupId) {
          const group = state.groups.find((g) => g.id === hover.groupId);
          const from =
            group?.tabs.findIndex((t) => documentKey(t.ref) === drag.key) ?? -1;
          if (from >= 0) {
            state.reorderTab(hover.groupId, drag.key, reorderTargetIndex(from, hover.index));
          }
        } else {
          state.moveTabToGroup(drag.key, drag.fromGroupId, hover.groupId, hover.index);
        }
      }
      hover?.element.classList.remove(hover.className);
      hoverRef.current = null;
      dragRef.current = null;
      document.body.style.removeProperty("cursor");
      setDragging(null);
    },
    [groups],
  );

  // Escape cancels an active drag (capture-phase, strip-drag parity).
  useEffect(() => {
    if (dragging === null) return undefined;
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      gesture.cancel();
      finishDrag(false);
    };
    // DISMISS-NET-EXEMPT: cancels an in-progress tab-drag gesture (transient
    // React state), not a floating menu/popover/modal surface — the
    // dismiss-registry-enrolment.test.ts scan requires this marker or a
    // registerDismissible() enrolment for every document-level keydown /
    // pointerdown listener it finds.
    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [dragging, gesture, finishDrag]);

  const setGhostElement = useCallback((element: HTMLDivElement | null): void => {
    ghostRef.current = element;
    if (element !== null) positionGhost(element, lastPointRef.current);
  }, []);

  /** Re-resolve the hovered insertion gap from the pointer position. */
  const updateHover = useCallback((x: number, y: number): void => {
    let target: HoverTarget | null = null;
    const bars = document.querySelectorAll<HTMLElement>(TAB_BAR_SELECTOR);
    for (const bar of bars) {
      const rect = bar.getBoundingClientRect();
      if (x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom) continue;
      const groupId = bar.getAttribute("data-group");
      if (groupId === null) continue;
      const tabs = Array.from(bar.querySelectorAll<HTMLElement>(TAB_SELECTOR));
      const index = insertionIndexForX(
        tabs.map((t) => {
          const r = t.getBoundingClientRect();
          return { left: r.left, right: r.right };
        }),
        x,
      );
      // The indicator lives on the tab at the gap (left edge), the last tab
      // (right edge) for a tail append, or the bar itself when it is empty.
      if (index < tabs.length) {
        target = { groupId, index, element: tabs[index], className: DROP_BEFORE_CLASS };
      } else if (tabs.length > 0) {
        target = { groupId, index, element: tabs[tabs.length - 1], className: DROP_AFTER_CLASS };
      } else {
        target = { groupId, index, element: bar, className: DROP_EMPTY_CLASS };
      }
      break;
    }

    const previous = hoverRef.current;
    if (
      previous !== null &&
      target !== null &&
      previous.element === target.element &&
      previous.className === target.className &&
      previous.index === target.index
    ) {
      return;
    }
    previous?.element.classList.remove(previous.className);
    target?.element.classList.add(target.className);
    hoverRef.current = target;
  }, []);

  const handlersFor = useCallback(
    (groupId: string, tab: EditorTab): TabDragHandlers => {
      const key = documentKey(tab.ref);
      const title = tab.ref.title;
      return {
        onPointerDown: (event) => {
          if (event.button !== 0 || !event.isPrimary) return;
          // A press on the close glyph is a close, never a drag (and must
          // not activate the tab on its way through).
          if ((event.target as HTMLElement).closest(".brink-tab-close") !== null) return;
          gesture.pointerDown(event.clientX, event.clientY);
          // Pressing a tab activates it before any drag starts (VS Code /
          // Zed behavior) — the dragged tab is always the active one.
          groups.getState().setActiveTab(groupId, key);
          // Capture so the threshold check and the whole drag keep receiving
          // moves outside the tab. (jsdom has no pointer capture — guard.)
          if (typeof event.currentTarget.setPointerCapture === "function") {
            event.currentTarget.setPointerCapture(event.pointerId);
          }
        },
        onPointerMove: (event) => {
          const result = gesture.pointerMove(event.clientX, event.clientY);
          if (result === "ignore") return;
          lastPointRef.current = { x: event.clientX, y: event.clientY };
          if (result === "start") {
            dragRef.current = { key, title, fromGroupId: groupId };
            document.body.style.cursor = "grabbing";
            setDragging(dragRef.current);
          }
          if (ghostRef.current !== null) {
            positionGhost(ghostRef.current, lastPointRef.current);
          }
          updateHover(event.clientX, event.clientY);
        },
        onPointerUp: () => {
          if (gesture.pointerUp() === "drop") finishDrag(true);
        },
        onPointerCancel: () => {
          gesture.cancel();
          if (dragRef.current !== null) finishDrag(false);
        },
      };
    },
    [groups, gesture, finishDrag, updateHover],
  );

  const consumeClickSuppression = useCallback(
    () => gesture.consumeClickSuppression(),
    [gesture],
  );

  return { dragging, setGhostElement, handlersFor, consumeClickSuppression };
}
