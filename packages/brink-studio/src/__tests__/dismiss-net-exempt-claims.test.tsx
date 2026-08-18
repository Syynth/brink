/**
 * Behavioural backing for the four `DISMISS-NET-EXEMPT` markers that
 * predate #2846 (#2846 point 2, modelled on the `SAVE-PATH` precedent
 * `docs/studio-shell-spec.md` §7.7.1: "a marker's justification is proven,
 * not just present" — #2571).
 *
 * `dismiss-registry-enrolment.test.ts` proves each marked call site EXISTS
 * and carries a non-empty reason; it cannot prove the reason is TRUE — a
 * future refactor that turned, say, `tab-drag.ts`'s drag-cancel into a
 * floating drag-preview surface would keep its exemption silently, and the
 * enrolment guard would stay green while the dismiss net grew a hole. Each
 * `describe` below is one marker's claim, made concrete against the real
 * production module (not a reimplementation) so a regression in the
 * UNDERLYING behaviour — not just a marker going missing — fails a test:
 *
 *  - `tab-drag.ts` / `strip-drag.ts`: "cancels an in-progress drag gesture
 *    (transient React state), not a floating menu/popover/modal surface."
 *    Proven by actually starting a drag through the hook's own pointer
 *    handlers, pressing Escape, and asserting the drag is CANCELLED (not
 *    committed — no store mutation) and the drag state clears. A surface
 *    that were secretly a floating menu would need `registerDismissible()`
 *    to be dismissed by the *global* net (orphan case, §7.7.3); this proves
 *    the opposite — its OWN capture-phase listener, not the net, is what
 *    reacts, and there is nothing left mounted afterward for the net to
 *    need to reach.
 *  - `regions.tsx`: "restores tool-window/editor-group maximize (layout
 *    store state), not a floating menu/popover/modal surface." Proven by
 *    mounting the real `ShellFrame`, maximizing an editor group through the
 *    real `editorGroups` store, pressing Escape, and asserting the store's
 *    `maximizedGroupId` clears — restoring layout state, which is what the
 *    marker claims, is exactly what "dismissing a surface" is not.
 *  - `ElementDropdown.tsx`: "arrow/Enter/shortcut navigation only — Escape
 *    dismissal is the wrapping Overlay's job, and Overlay is enrolled."
 *    Proven end-to-end against the real component: Escape must NOT reach
 *    `onSelect` (this component's own handler truly ignores it) AND MUST
 *    reach `onDismiss` (the wrapping, already-enrolled `Overlay` truly
 *    handles it) — either half failing alone would falsify the claim.
 */

import { describe, it, expect, afterEach, beforeAll, vi } from "vitest";
import { act, createElement, type PointerEvent as ReactPointerEvent } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderHook } from "./render-hook.js";
import {
  ShellProvider,
  createEditorGroupsStore,
  createShellLayoutStore,
  useShell,
  useShellLayout,
  useEditorGroups,
  useStripDrag,
  useTabDrag,
  ShellFrame,
  CommandRegistry,
  DocumentTypeRegistry,
  resetDismissRegistryForTests,
  type DocumentRef,
  type DocumentViewProps,
  type EditorGroupsStore,
  type ToolWindowDescriptor,
} from "@brink/studio-shell";
import { ElementDropdown } from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

// jsdom has no ResizeObserver; GroupTabBar/EditorArea (mounted by every
// editor group, including the maximized one ShellFrame renders below) uses
// one to compute the overflow-chevron state (#278) — see
// editor-area-maximize-paint.test.tsx for the same stub pattern.
beforeAll(() => {
  if (typeof globalThis.ResizeObserver === "undefined") {
    class ResizeObserverStub {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver =
      ResizeObserverStub;
  }
  if (typeof Element.prototype.scrollIntoView === "undefined") {
    Element.prototype.scrollIntoView = () => {};
  }
});

function ref(docId: string): DocumentRef {
  return { typeId: "test-doc", docId, title: docId };
}

function escapeEvent(): KeyboardEvent {
  return new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
}

// ── tab-drag.ts: Escape cancels the drag gesture, does not commit ──────

describe("tab-drag.ts Escape-cancel (DISMISS-NET-EXEMPT: transient drag state, not a surface)", () => {
  it("Escape mid-drag clears dragging state and commits no reorder/move", () => {
    const groups = createEditorGroupsStore();
    groups.getState().openDocument(ref("a.ink"));
    groups.getState().openDocument(ref("b.ink"), { group: "split-right" });
    const groupId = groups.getState().groups[0].id;
    const tab = groups.getState().groups[0].tabs[0];

    const { result } = renderHook(() => useTabDrag(groups));

    const reorderSpy = vi.spyOn(groups.getState(), "reorderTab");
    const moveSpy = vi.spyOn(groups.getState(), "moveTabToGroup");

    const target = document.createElement("div"); // real node: closest() needs one
    act(() => {
      result.current.handlersFor(groupId, tab).onPointerDown({
        button: 0,
        isPrimary: true,
        clientX: 0,
        clientY: 0,
        target,
        currentTarget: target,
        pointerId: 1,
      } as unknown as ReactPointerEvent<HTMLDivElement>);
    });
    act(() => {
      // Past the 5px threshold: starts the drag.
      result.current.handlersFor(groupId, tab).onPointerMove({
        clientX: 0,
        clientY: 20,
      } as unknown as ReactPointerEvent<HTMLDivElement>);
    });
    expect(result.current.dragging).not.toBeNull();

    act(() => {
      document.dispatchEvent(escapeEvent());
    });

    expect(result.current.dragging).toBeNull();
    expect(reorderSpy).not.toHaveBeenCalled();
    expect(moveSpy).not.toHaveBeenCalled();
  });
});

// ── strip-drag.ts: Escape cancels the drag gesture, does not commit ────

describe("strip-drag.ts Escape-cancel (DISMISS-NET-EXEMPT: transient drag state, not a surface)", () => {
  function descriptor(id: string): ToolWindowDescriptor {
    return {
      id,
      title: id,
      icon: null,
      defaultPlacement: { dock: "left", section: "start" },
      defaultOpen: true,
      component: () => null,
    };
  }

  it("Escape mid-drag clears dragging state and commits no re-dock", () => {
    const layout = createShellLayoutStore();
    layout.getState().syncFromRegistry([descriptor("player")]);

    const { result } = renderHook(() => useStripDrag(layout));
    const moveSpy = vi.spyOn(layout.getState(), "moveToolWindow");

    const target = { setPointerCapture: undefined } as unknown as HTMLButtonElement;
    act(() => {
      result.current.handlersFor(descriptor("player")).onPointerDown({
        button: 0,
        isPrimary: true,
        clientX: 0,
        clientY: 0,
        target,
        currentTarget: target,
        pointerId: 1,
      } as unknown as ReactPointerEvent<HTMLButtonElement>);
    });
    act(() => {
      result.current.handlersFor(descriptor("player")).onPointerMove({
        clientX: 0,
        clientY: 20,
      } as unknown as ReactPointerEvent<HTMLButtonElement>);
    });
    expect(result.current.dragging).not.toBeNull();

    act(() => {
      document.dispatchEvent(escapeEvent());
    });

    expect(result.current.dragging).toBeNull();
    expect(moveSpy).not.toHaveBeenCalled();
  });
});

// ── regions.tsx: Escape restores maximize (layout state), not a surface ─

describe("regions.tsx maximize-restore (DISMISS-NET-EXEMPT: layout state, not a surface)", () => {
  let root: Root | null = null;
  let container: HTMLDivElement | null = null;

  afterEach(() => {
    if (root) act(() => root!.unmount());
    container?.remove();
    root = null;
    container = null;
    resetDismissRegistryForTests();
  });

  function TestDoc({ doc }: DocumentViewProps) {
    return createElement("div", { "data-testid": "doc-body" }, doc.docId);
  }

  function documents(): DocumentTypeRegistry {
    const registry = new DocumentTypeRegistry();
    registry.register({ id: "test-doc", component: TestDoc });
    return registry;
  }

  function mount(editorGroups: EditorGroupsStore): void {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    const commands = new CommandRegistry();
    act(() => {
      root!.render(
        <ShellProvider commands={commands} editorGroups={editorGroups} documents={documents()}>
          <ShellFrame />
        </ShellProvider>,
      );
    });
  }

  it("Escape restores an editor-group maximize and touches nothing else", () => {
    const groups = createEditorGroupsStore();
    groups.getState().openDocument(ref("a.ink"));
    groups.getState().openDocument(ref("b.ink"), { group: "split-right" });

    mount(groups);

    act(() => {
      groups.getState().toggleMaximizeGroup();
    });
    expect(groups.getState().maximizedGroupId).not.toBeNull();

    act(() => {
      document.dispatchEvent(escapeEvent());
    });

    expect(groups.getState().maximizedGroupId).toBeNull();
  });
});

// ── ElementDropdown.tsx: Escape is the wrapping Overlay's job ──────────

describe("ElementDropdown.tsx (DISMISS-NET-EXEMPT: nav only — Escape is the wrapping Overlay's job)", () => {
  let root: Root | null = null;
  let container: HTMLDivElement | null = null;
  let anchor: HTMLButtonElement | null = null;

  afterEach(() => {
    if (root) act(() => root!.unmount());
    container?.remove();
    anchor?.remove();
    root = null;
    container = null;
    anchor = null;
    resetDismissRegistryForTests();
  });

  it("Escape reaches the wrapping Overlay's onDismiss, not this component's own onSelect", () => {
    resetDismissRegistryForTests();
    anchor = document.createElement("button");
    document.body.appendChild(anchor);
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    const onSelect = vi.fn();
    const onDismiss = vi.fn();
    act(() => {
      root!.render(
        createElement(ElementDropdown, { open: true, anchor, onSelect, onDismiss }),
      );
    });

    act(() => {
      document.dispatchEvent(escapeEvent());
    });

    // If ElementDropdown's own handleKeyDown were intercepting Escape (a
    // regression that would falsify the marker's "nav only" half), this
    // would misfire — a shortcut-letter branch could even call onSelect.
    expect(onSelect).not.toHaveBeenCalled();
    // If the wrapping Overlay were NOT actually handling Escape (a
    // regression that would falsify the marker's "Overlay's job" half —
    // e.g. Overlay unmounted, or its own listener broken), this would never
    // fire and the dropdown would be the #279 stuck-menu case.
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });
});
