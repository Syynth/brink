/**
 * Binder v2 Files/Structure toggle (#3036 — compare
 * `docs/design/binder-v2/{Main,Structure}.dc.html`): files-only is the
 * ruled default (symbol rows are the noise); Structure mode reveals
 * knot/stitch rows; expand/collapse-all drive the collapsed set.
 */
import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const sym = (name: string, kind: string, children: unknown[] = []) => ({
  name,
  kind,
  start: 0,
  end: 1,
  children,
});

const OUTLINE: FileOutline[] = [
  {
    path: "main.ink",
    symbols: [sym("intro", "knot", [sym("first", "stitch")])] as never,
    mounted: false,
  },
  { path: "scenes/harbour.ink", symbols: [sym("dock", "knot")] as never, mounted: false },
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

function rowKeys(): string[] {
  return [...container!.querySelectorAll("[data-binder-row-key]")].map(
    (el) => el.getAttribute("data-binder-row-key") ?? "",
  );
}

describe("Binder Files/Structure toggle (#3036)", () => {
  it("hides symbol rows by default (Files mode), shows them in Structure mode", () => {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
    mountBinder(store);

    expect(rowKeys()).toContain("main.ink");
    expect(rowKeys()).not.toContain("main.ink::intro");

    act(() => {
      container!
        .querySelector<HTMLButtonElement>(".brink-binder-mode-toggle button[title='Structure']")
        ?.click();
    });
    expect(rowKeys()).toContain("main.ink::intro");
    expect(rowKeys()).toContain("main.ink::intro::first");

    act(() => {
      container!
        .querySelector<HTMLButtonElement>(".brink-binder-mode-toggle button[title='Files']")
        ?.click();
    });
    expect(rowKeys()).not.toContain("main.ink::intro");
  });

  it("collapse-all collapses every expandable row; expand-all reopens", () => {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
    store.getState().toggleStructureMode(); // Structure: symbols count too
    mountBinder(store);

    act(() => {
      container!
        .querySelector<HTMLButtonElement>(".brink-binder-tool[title='Collapse all']")
        ?.click();
    });
    // The folder is collapsed (its child gone) and main.ink's knot is hidden.
    expect(rowKeys()).not.toContain("scenes/harbour.ink");
    expect(rowKeys()).not.toContain("main.ink::intro");
    expect(store.getState().collapsed.size).toBeGreaterThan(0);

    act(() => {
      container!
        .querySelector<HTMLButtonElement>(".brink-binder-tool[title='Expand all']")
        ?.click();
    });
    expect(rowKeys()).toContain("scenes/harbour.ink");
    expect(rowKeys()).toContain("main.ink::intro");
    expect(store.getState().collapsed.size).toBe(0);
  });

  it("renders SVG icons, not glyph characters (#3037)", () => {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
    mountBinder(store);
    const icons = container!.querySelectorAll(".brink-binder-icon svg");
    expect(icons.length).toBeGreaterThan(0);
    // None of the retired glyphs appear anywhere in the tree.
    for (const glyph of ["📄", "📁", "◆", "◇", "ƒ", "📚"]) {
      expect(container!.textContent).not.toContain(glyph);
    }
  });
});
