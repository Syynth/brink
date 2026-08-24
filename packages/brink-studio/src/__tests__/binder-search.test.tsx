/**
 * Binder search (#3040 — compare `docs/design/binder-v2/Search.dc.html`):
 * one query over file names and structural names; matches keep their file
 * context; a match's symbols reveal in BOTH modes; clear restores. The
 * #tag namespace is deliberately absent — the tag data does not exist at
 * any layer yet (#474 owns wiring it through HIR → format → wasm).
 */
import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, StoreProvider, filterOutline } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const sym = (name: string, kind: string, children: unknown[] = []) => ({
  name,
  kind,
  start: 0,
  end: 1,
  full_start: 0,
  full_end: 100,
  children,
});

const OUTLINE: FileOutline[] = [
  {
    path: "codetta.ink",
    symbols: [sym("interrogation", "knot"), sym("harbour", "knot")] as never,
    mounted: false,
  },
  {
    path: "menus/interludes.ink",
    symbols: [sym("pause_menu", "knot")] as never,
    mounted: false,
  },
  { path: "menus/options.ink", symbols: [] as never, mounted: false },
];

describe("filterOutline (#3040)", () => {
  it("matches file basenames and keeps the file whole", () => {
    const out = filterOutline(OUTLINE, "interludes");
    expect(out.map((f) => f.path)).toEqual(["menus/interludes.ink"]);
    expect(out[0]?.symbols).toHaveLength(1);
  });
  it("matches symbol names, narrowing the file to the matching subtree", () => {
    const out = filterOutline(OUTLINE, "interrog");
    expect(out.map((f) => f.path)).toEqual(["codetta.ink"]);
    expect(out[0]?.symbols.map((s) => s.name)).toEqual(["interrogation"]);
  });
  it("a stitch match survives as its knot's context", () => {
    const nested: FileOutline[] = [
      {
        path: "a.ink",
        symbols: [sym("outer", "knot", [sym("findme", "stitch")])] as never,
        mounted: false,
      },
    ];
    const out = filterOutline(nested, "findme");
    expect(out[0]?.symbols[0]?.name).toBe("outer");
    expect(out[0]?.symbols[0]?.children.map((c) => c.name)).toEqual(["findme"]);
  });
  it("empty query is the identity", () => {
    expect(filterOutline(OUTLINE, "  ")).toBe(OUTLINE);
  });
});

describe("Binder search UI (#3040)", () => {
  let root: Root | null = null;
  let container: HTMLDivElement | null = null;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    root = null;
    container = null;
  });

  function mountBinder() {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    act(() => {
      root!.render(createElement(StoreProvider, { store, children: createElement(Binder) }));
    });
    return store;
  }

  function keys(): string[] {
    return [...container!.querySelectorAll("[data-binder-row-key]")].map(
      (el) => el.getAttribute("data-binder-row-key") ?? "",
    );
  }

  function type(value: string): void {
    act(() => {
      const input = container!.querySelector<HTMLInputElement>(".brink-binder-search-input");
      if (input === null) throw new Error("search input missing");
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      setter?.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  }

  it("a symbol query reveals the file AND the matching symbol, even in Files mode", () => {
    mountBinder();
    act(() => {
      container!
        .querySelector<HTMLButtonElement>(".brink-binder-tool[title='Search binder']")
        ?.click();
    });
    type("interrog");
    const k = keys();
    expect(k).toContain("codetta.ink");
    expect(k).toContain("codetta.ink::interrogation");
    expect(k).not.toContain("codetta.ink::harbour");
    expect(k).not.toContain("menus/options.ink");
  });

  it("Escape clears and restores the full tree, symbols hidden again (Files mode)", () => {
    mountBinder();
    act(() => {
      container!
        .querySelector<HTMLButtonElement>(".brink-binder-tool[title='Search binder']")
        ?.click();
    });
    type("interrog");
    act(() => {
      container!
        .querySelector<HTMLInputElement>(".brink-binder-search-input")
        ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    const k = keys();
    expect(k).toContain("menus/options.ink");
    expect(k).not.toContain("codetta.ink::interrogation");
    expect(container!.querySelector(".brink-binder-search")).toBeNull();
  });
});
