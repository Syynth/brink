/**
 * Diagnostics marks + the pinned brink.toml row (#3041/#3042 — compare
 * `docs/design/binder-v2/{Main,Structure}.dc.html`): file rows carry
 * error/warning counts (a file sums its diagnostics; a symbol shows its
 * own), and brink.toml leaves the tree for a dedicated pinned row that
 * opens it.
 */
import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Binder, StoreProvider, fileMarks, symbolMarks } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { Diagnostic, FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const knot = {
  name: "intro",
  kind: "knot",
  start: 10,
  end: 15,
  full_start: 10,
  full_end: 60,
  children: [],
};

const OUTLINE: FileOutline[] = [
  { path: "main.ink", symbols: [knot] as never, mounted: false },
  { path: "clean.ink", symbols: [], mounted: false },
  { path: "brink.toml", symbols: [], mounted: false },
];

const DIAGS: Diagnostic[] = [
  { start: 20, end: 22, message: "boom", severity: "Error", file: "main.ink" },
  { start: 80, end: 82, message: "meh", severity: "Warning", file: "main.ink" },
  { start: 5, end: 6, message: "fyi", severity: "Info", file: "main.ink" },
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

describe("mark helpers (#3041)", () => {
  it("files sum Error+Warning; Info/Hint never mark", () => {
    const marks = fileMarks(DIAGS);
    expect(marks.get("main.ink")).toEqual({ errors: 1, warnings: 1 });
    expect(marks.has("clean.ink")).toBe(false);
  });
  it("a symbol counts only diagnostics inside its full body", () => {
    expect(symbolMarks(DIAGS, "main.ink", knot as never)).toEqual({ errors: 1, warnings: 0 });
  });
});

describe("Binder marks + pinned config (#3041/#3042)", () => {
  function seeded() {
    const store = createStudioStore();
    store.getState().setCompileResult(OUTLINE, { errors: 1, warnings: 1 }, DIAGS, null);
    return store;
  }

  it("renders counts on the offending file row only", () => {
    mountBinder(seeded());
    const row = [...container!.querySelectorAll("[data-binder-row-key]")].find(
      (el) => el.getAttribute("data-binder-row-key") === "main.ink",
    );
    expect(row?.querySelector(".brink-mark-error")?.textContent).toBe("1");
    expect(row?.querySelector(".brink-mark-warning")?.textContent).toBe("1");
    const clean = [...container!.querySelectorAll("[data-binder-row-key]")].find(
      (el) => el.getAttribute("data-binder-row-key") === "clean.ink",
    );
    expect(clean?.querySelector(".brink-mark")).toBeNull();
  });

  it("brink.toml leaves the tree and renders as the pinned config row", () => {
    mountBinder(seeded());
    const configRows = [...container!.querySelectorAll("[data-binder-row-key='brink.toml']")];
    expect(configRows).toHaveLength(1);
    expect(configRows[0]?.classList.contains("brink-binder-config-row")).toBe(true);
  });

  it("clicking the pinned row opens brink.toml", () => {
    const store = seeded();
    const opened: unknown[] = [];
    store.setState({
      openTarget: ((target: unknown) => {
        opened.push(target);
      }) as never,
    });
    mountBinder(store);
    act(() => {
      container!
        .querySelector<HTMLElement>(".brink-binder-config-row")
        ?.click();
    });
    expect(opened).toEqual([{ kind: "file", path: "brink.toml" }]);
  });

  it("no config, no pinned row", () => {
    const store = createStudioStore();
    store
      .getState()
      .setCompileResult(OUTLINE.filter((f) => f.path !== "brink.toml"), { errors: 0, warnings: 0 }, [], null);
    mountBinder(store);
    expect(container!.querySelector(".brink-binder-config-row")).toBeNull();
  });
});
