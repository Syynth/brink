/**
 * Binder scope marks + the ink-project Library gate (#3014/#3021 —
 * compare `docs/design/project-open-flow/Binder.dc.html`):
 *
 * - the entry file carries the `entry` badge (the project's anchor);
 * - a source file outside the compile closure renders dimmed with a
 *   `not included` badge — on disk, not in the story;
 * - the Library section (mounted stdlib, #2306/#2343) is hidden entirely
 *   for an ink project, where the compiler provably excludes the mounted
 *   `.brink` stdlib from the closure (`brink-environment`'s
 *   `stdlib_mount_is_manifest_only_for_an_ink_entry`), and kept for a
 *   native entry (and before any compile, when the dialect is unknown).
 */
import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "main.ink", symbols: [], mounted: false },
  { path: "offcuts.ink", symbols: [], mounted: false },
  { path: "scenes/harbour.ink", symbols: [], mounted: false },
  { path: "std/screenplay.brink", symbols: [], mounted: true },
];

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mountBinder(store: ReturnType<typeof createStudioStore>) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(Binder) }));
  });
}

function seededStore(entry: string | null, closure: string[]) {
  const store = createStudioStore();
  store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
  store.getState().setClosureFiles(closure);
  store.getState().setEntryFile(entry);
  return store;
}

function rowFor(path: string): HTMLElement | null {
  for (const el of container!.querySelectorAll("[data-binder-row-key]")) {
    if (el.getAttribute("data-binder-row-key") === path) return el as HTMLElement;
  }
  return null;
}

describe("Binder scope marks (#3021)", () => {
  it("badges the entry and dims a not-included file", () => {
    mountBinder(seededStore("main.ink", ["main.ink", "scenes/harbour.ink"]));

    const entryRow = rowFor("main.ink");
    expect(entryRow?.querySelector(".brink-binder-badge-entry")?.textContent).toBe("entry");
    expect(entryRow?.classList.contains("brink-binder-dimmed")).toBe(false);

    const offcuts = rowFor("offcuts.ink");
    expect(offcuts?.classList.contains("brink-binder-dimmed")).toBe(true);
    expect(offcuts?.querySelector(".brink-binder-badge-muted")?.textContent).toBe("not included");

    // In the closure, not the entry: no badge, no dimming.
    const harbour = rowFor("scenes/harbour.ink");
    expect(harbour?.querySelector(".brink-binder-badge")).toBeNull();
    expect(harbour?.classList.contains("brink-binder-dimmed")).toBe(false);
  });

  it("marks nothing before the first compile — an empty closure asserts nothing", () => {
    mountBinder(seededStore(null, []));
    expect(container!.querySelector(".brink-binder-badge")).toBeNull();
    expect(container!.querySelector(".brink-binder-dimmed")).toBeNull();
  });
});

describe("Library gate for ink projects (#3014)", () => {
  it("hides the Library section when the entry is .ink", () => {
    mountBinder(seededStore("main.ink", ["main.ink"]));
    expect(container!.querySelector(".brink-binder-library-section")).toBeNull();
  });

  it("keeps the Library for a native entry, and before any compile", () => {
    mountBinder(seededStore("mod.brink", ["mod.brink"]));
    expect(container!.querySelector(".brink-binder-library-section")).not.toBeNull();

    act(() => root?.unmount());
    container?.remove();

    mountBinder(seededStore(null, []));
    expect(container!.querySelector(".brink-binder-library-section")).not.toBeNull();
  });
});
