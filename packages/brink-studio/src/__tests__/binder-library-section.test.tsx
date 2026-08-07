/**
 * Binder "Library" section (issue #2306/#2343, "Mounted stdlib presents as
 * a read-only library node", presentation half): mounted `std/` files show
 * as a visually distinct, collapsed-by-default section, separate from the
 * project's own file tree — browsable (expand/collapse, open on click) but
 * with no drag/rename/delete/new-file affordances.
 *
 * The pure tree-building logic (`buildBinderTree`) is already covered by
 * `binder-tree.test.ts`; this file covers the section's own rendering and
 * interaction, which lives in `Binder.tsx` itself.
 */

import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, StoreProvider, LIBRARY_ROW_KEY } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "main.ink", symbols: [], mounted: false },
  { path: "scenes/intro.ink", symbols: [], mounted: false },
  { path: "std/conventions/screenplay.brink", symbols: [], mounted: true },
];

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(store: ReturnType<typeof createStudioStore>) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(Binder) }));
  });
}

/**
 * Find a row by its exact `data-binder-row-key` value via `getAttribute` +
 * strict equality, not a `[attr="value"]` CSS selector: `LIBRARY_ROW_KEY`
 * embeds a NUL byte (`\u0000`, the same sentinel idiom `document-sessions.ts`'s
 * `slotId` uses) so it can never collide with a real file/folder path — but
 * per the CSS Syntax spec, a NUL inside a selector's string token is
 * replaced with U+FFFD during tokenizing, so a `[attr="\u0000library"]`
 * selector can never match an attribute whose real value contains the
 * literal NUL. `getAttribute` returns the raw stored string with no such
 * reparsing, so a plain `===` compare is the correct tool here.
 */
function findRow(key: string): HTMLElement {
  for (const el of container!.querySelectorAll("[data-binder-row-key]")) {
    if (el.getAttribute("data-binder-row-key") === key) return el as HTMLElement;
  }
  throw new Error(`row not found: ${JSON.stringify(key)}`);
}

function libraryRow(): HTMLElement {
  return findRow(LIBRARY_ROW_KEY);
}

/** Click the Library row's chevron (`onChevronClick`, called synchronously
 *  with `stopPropagation` — unlike the row's own `onClick`, which the real
 *  Binder debounces 200ms behind a `setTimeout` to disambiguate a single
 *  click from a double-click). Toggles `libraryExpanded` immediately, so
 *  the test doesn't need fake timers. */
function toggleLibraryViaChevron(): void {
  const chevron = libraryRow().querySelector(".brink-binder-chevron");
  if (chevron === null) throw new Error("Library row has no chevron");
  chevron.dispatchEvent(new MouseEvent("click", { bubbles: true }));
}

describe("Binder: Library section", () => {
  it("shows a Library row, distinct from the project's own file tree", () => {
    const store = createStudioStore();
    store.setState({ outline: OUTLINE });
    mount(store);

    expect(container!.textContent).toContain("Library");
    // The real project file tree contains only the non-mounted paths.
    expect(container!.textContent).toContain("main.ink");
    expect(container!.textContent).toContain("intro.ink");
  });

  it("is collapsed by default: the mounted file's name is not in the DOM at all", () => {
    const store = createStudioStore();
    store.setState({ outline: OUTLINE });
    mount(store);

    expect(container!.textContent).not.toContain("screenplay.brink");
  });

  it("expands on click, revealing the mounted file, and collapses again on a second click", () => {
    const store = createStudioStore();
    store.setState({ outline: OUTLINE });
    mount(store);

    act(() => {
      toggleLibraryViaChevron();
    });
    expect(container!.textContent).toContain("screenplay.brink");

    act(() => {
      toggleLibraryViaChevron();
    });
    expect(container!.textContent).not.toContain("screenplay.brink");
  });

  it("renders no Library section at all when the project has no mounted files", () => {
    const store = createStudioStore();
    store.setState({ outline: OUTLINE.filter((f) => !f.mounted) });
    mount(store);

    expect(container!.textContent).not.toContain("Library");
  });

  it("a mounted file row is not draggable and opens the file on click (read-only browsing, not a dead row)", () => {
    const store = createStudioStore();
    const opened: unknown[] = [];
    store.setState({ outline: OUTLINE, _openTarget: (target) => opened.push(target) });
    mount(store);

    act(() => {
      toggleLibraryViaChevron();
    });

    const fileRow = container!.querySelector(
      '[data-binder-row-key="std/conventions/screenplay.brink"]',
    );
    expect(fileRow).not.toBeNull();
    expect(fileRow!.getAttribute("draggable")).toBe("false");

    // Double-click bypasses the single-click debounce entirely (the Binder
    // fires `onDoubleClick` immediately, canceling the pending single-click
    // timer) — the real user gesture a "open this read-only file" row is
    // most naturally exercised by, without needing fake timers.
    act(() => {
      fileRow!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    });
    expect(opened).toEqual([{ kind: "file", path: "std/conventions/screenplay.brink" }]);
  });

  it("ctrl/cmd-clicking a Library row never joins the shared selection (issue #2343 review finding)", async () => {
    // A Library row has no visual selected state (`isSelected` is hardcoded
    // `false`) and no drag/rename/delete affordances, but before this fix a
    // ctrl/cmd-click still routed through the same `handleRowClick` as an
    // ordinary project-file row and called `selectKey`, silently adding the
    // mounted path to `selectedKeys` — invisibly, since the row never
    // reflects it — where a later drag of a co-selected project file would
    // expand to include it and get refused by `applyMoveResult`.
    //
    // `BinderRow`'s own `onClick` handler debounces 200ms behind a real
    // `setTimeout` to disambiguate a single click from a double-click (see
    // the module doc above `toggleLibraryViaChevron`) — this test waits out
    // that real delay (unlike the file's other click test, which sidesteps
    // the debounce entirely with a `dblclick`) so the assertion runs after
    // the debounced handler actually fires.
    const store = createStudioStore();
    store.setState({ outline: OUTLINE });
    mount(store);

    act(() => {
      toggleLibraryViaChevron();
    });

    const fileRow = container!.querySelector(
      '[data-binder-row-key="std/conventions/screenplay.brink"]',
    );
    expect(fileRow).not.toBeNull();

    act(() => {
      fileRow!.dispatchEvent(
        new MouseEvent("click", { bubbles: true, ctrlKey: true, metaKey: true }),
      );
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 250));
    });

    expect(store.getState().selectedKeys.has("std/conventions/screenplay.brink")).toBe(false);
  });
});
