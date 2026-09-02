/**
 * Issue #3351: folder drag-reorder in the Binder silently failed in WebKit
 * (Tauri desktop / Safari) while Chromium worked. WebKit's HTML5 drag-and-drop
 * requires `preventDefault()` on BOTH `dragenter` and `dragover` for an
 * element to remain a valid drop target — Chromium tolerates `dragover`
 * alone. `Binder.tsx` wired only `onDragOver`/`onDrop` to its rows; there was
 * no `onDragEnter` handler at all, so WebKit never registered a folder row as
 * a drop target and the drop silently did nothing.
 *
 * jsdom does not implement a real OS-level drag session — it will still
 * dispatch a `drop` event to a target even if a prior `dragenter`/`dragover`
 * never called `preventDefault()`, so a "did the reorder happen" assertion
 * alone would pass whether or not the fix is present (it's proving nothing
 * about the actual browser contract WebKit enforces). The two assertions that
 * DO pin the real contract, and fail without the fix:
 *
 *   1. `dragenter` on a valid drop-target row must call `preventDefault()`
 *      (the literal WebKit requirement named in #3351).
 *   2. The drop-target visual state (the insertion line) must be computed
 *      from `dragenter` alone, without waiting for a following `dragover` —
 *      proving a real handler ran on enter, not just that the browser
 *      happened to fire `dragover` next in this environment.
 */

import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { ProjectSession } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

// Two root-level folders, each with one file — "alpha/" and "beta/" are
// siblings in the root container ("", per `containerOf`), which is exactly
// the same-container folder reorder #3038 introduced and #3351 broke in
// WebKit.
const OUTLINE: FileOutline[] = [
  { path: "alpha/one.ink", symbols: [], mounted: false },
  { path: "beta/two.ink", symbols: [], mounted: false },
];

function stubProject(): ProjectSession {
  return { canRenameFiles: () => true } as unknown as ProjectSession;
}

/** A minimal fake `DataTransfer` — jsdom's real one does not support the
 *  full drag-session contract, and the production handlers only ever call
 *  `setData`/`getData`/read-write `effectAllowed`/`dropEffect`. */
function fakeDataTransfer() {
  const store: Record<string, string> = {};
  return {
    effectAllowed: "none",
    dropEffect: "none",
    types: [] as string[],
    setData(format: string, value: string) {
      store[format] = value;
    },
    getData(format: string) {
      return store[format] ?? "";
    },
  };
}

/** Build a native, bubbling, cancelable drag event with a fake
 *  `dataTransfer` attached — jsdom's `DragEvent` constructor does not accept
 *  `dataTransfer` in its init dict, so it's defined directly on the event,
 *  the same workaround Testing Library's `fireEvent` uses internally. */
function dragEvent(type: string, dataTransfer: ReturnType<typeof fakeDataTransfer>): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: dataTransfer, configurable: true });
  return event;
}

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount() {
  const store = createStudioStore();
  store.setState({ outline: OUTLINE, _project: stubProject() });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(Binder) }));
  });
  return store;
}

function findRow(key: string): HTMLElement {
  for (const el of container!.querySelectorAll("[data-binder-row-key]")) {
    if (el.getAttribute("data-binder-row-key") === key) return el as HTMLElement;
  }
  throw new Error(`row not found: ${JSON.stringify(key)}`);
}

describe("Binder folder drag-reorder: WebKit dragenter contract (#3351)", () => {
  it("preventDefault()s a dragenter on a valid same-container drop target, not just dragover", () => {
    mount();
    const alpha = findRow("alpha/");
    const beta = findRow("beta/");
    const dt = fakeDataTransfer();

    act(() => {
      alpha.dispatchEvent(dragEvent("dragstart", dt));
    });

    let enterEvent!: Event;
    act(() => {
      enterEvent = dragEvent("dragenter", dt);
      beta.dispatchEvent(enterEvent);
    });

    // The literal WebKit requirement from #3351: without a wired
    // `onDragEnter` calling `preventDefault()`, this stays false and WebKit
    // never treats `beta` as a valid drop target — the drop silently no-ops.
    expect(enterEvent.defaultPrevented).toBe(true);
  });

  it("computes the drop-target insertion line from dragenter alone, before any dragover fires", () => {
    mount();
    const alpha = findRow("alpha/");
    const beta = findRow("beta/");
    const dt = fakeDataTransfer();

    act(() => {
      alpha.dispatchEvent(dragEvent("dragstart", dt));
    });
    act(() => {
      beta.dispatchEvent(dragEvent("dragenter", dt));
    });

    // A real handler ran on `dragenter` and set drop-target state: the
    // insertion-line marker renders adjacent to `beta`'s row. Before the fix,
    // no `onDragEnter` handler existed at all, so no drop-line ever appears
    // here — this assertion is false with the pre-fix production code.
    const betaContainer = beta.parentElement;
    expect(betaContainer).not.toBeNull();
    const dropLine = betaContainer!.querySelector(".brink-binder-drop-line");
    expect(dropLine).not.toBeNull();
  });

  it("still prevents default on dragover (unchanged, sanity pin)", () => {
    mount();
    const alpha = findRow("alpha/");
    const beta = findRow("beta/");
    const dt = fakeDataTransfer();

    act(() => {
      alpha.dispatchEvent(dragEvent("dragstart", dt));
    });

    let overEvent!: Event;
    act(() => {
      overEvent = dragEvent("dragover", dt);
      beta.dispatchEvent(overEvent);
    });

    expect(overEvent.defaultPrevented).toBe(true);
  });
});
