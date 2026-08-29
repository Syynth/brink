/**
 * Right-click a Problems row to suppress that diagnostic (#3148).
 *
 * The text edits are covered by `suppress-diagnostic.test.ts`; this is
 * about the MENU — which items appear for which diagnostic, and that
 * picking one writes to the right file.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ProblemsContextMenu, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { Diagnostic, FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "brink.toml", symbols: [], mounted: false },
  { path: "main.ink", symbols: [], mounted: false },
];

const REGISTRY = [
  { code: "E035", default_severity: "warning" },
  { code: "E001", default_severity: "error" },
];

const STORY = `=== intro ===
VAR roll = 0
`;
const CONFIG = `[project]\nentry = "main.ink"\n`;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(diagnostic: Diagnostic, opts: { withConfig?: boolean } = {}) {
  const files = new Map<string, string>([["main.ink", STORY]]);
  if (opts.withConfig !== false) files.set("brink.toml", CONFIG);
  const applied: [string, string][] = [];

  const project = {
    getDiagnosticRegistry: () => REGISTRY,
    getSession: () => ({ getFileSource: (p: string) => files.get(p) ?? null }),
    applyEdit: (p: string, next: string) => {
      files.set(p, next);
      applied.push([p, next]);
      return true;
    },
  };

  const store = createStudioStore();
  store
    .getState()
    .setCompileResult(
      opts.withConfig === false ? OUTLINE.filter((f) => f.path !== "brink.toml") : OUTLINE,
      { errors: 0, warnings: 1 },
      [],
      null,
    );
  store.setState({
    _project: project as never,
    _documents: { refreshExternal: vi.fn(), triggerCompile: vi.fn() } as never,
  });

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(StoreProvider, {
        store,
        children: createElement(ProblemsContextMenu, {
          target: { x: 0, y: 0, diagnostic },
          onClose: () => {},
        }),
      }),
    );
  });
  return { files, applied, store };
}

const diag = (over: Partial<Diagnostic> = {}): Diagnostic => ({
  start: STORY.indexOf("VAR roll"),
  end: STORY.indexOf("VAR roll") + 3,
  message: "name shadows a built-in function: `roll`",
  severity: "Warning",
  code: "E035",
  file: "main.ink",
  ...over,
});

const labels = (): string[] =>
  [...container!.querySelectorAll(".brink-context-menu-item")].map((e) => e.textContent ?? "");

const click = (label: string): void => {
  const item = [...container!.querySelectorAll(".brink-context-menu-item")].find(
    (e) => e.textContent === label,
  );
  act(() => (item as HTMLElement | undefined)?.click());
};

describe("which items appear", () => {
  it("offers every scope, narrowest first, each labelled for what it does", () => {
    // The blanket file gesture is its OWN item (#3259). One item that said
    // "Suppress E035 in this file" while silencing the whole file is what
    // this split fixes — the label and the effect now agree, both times.
    mount(diag());
    expect(labels()).toEqual([
      "Suppress E035 on this line",
      "Suppress E035 in this file",
      "Suppress all diagnostics in this file",
      "Suppress E035 in this project",
      "Configure E035…",
    ]);
  });

  it("Configure opens the Diagnostics section, not the config file", () => {
    // It used to route through the `brink.toml` opener, which lands on
    // Project — the wrong place to arrive from "configure this diagnostic".
    const { store } = mount(diag());
    click("Configure E035…");
    expect(store.getState().settingsSection).toBe("diagnostics");
  });

  it("offers no suppression for an error-tier code", () => {
    // Every channel refuses it, so offering the gesture would be offering
    // a no-op. Configure stays: you can still go look at it.
    mount(diag({ code: "E001", severity: "Error" }));
    expect(labels().filter((l) => l.startsWith("Suppress"))).toEqual([]);
    expect(labels()).toContain("Configure E001…");
  });

  it("offers no suppression for a code it cannot look up", () => {
    // Unknown severity — better to omit the gesture than write a directive
    // the compiler may reject.
    mount(diag({ code: "E999" }));
    expect(labels().filter((l) => l.startsWith("Suppress"))).toEqual([]);
  });

  it("disables the project scope when there is no brink.toml", () => {
    // Disabled rather than hidden: the gesture exists, the project just has
    // nowhere to record it — hiding it would read as "cannot be suppressed".
    mount(diag(), { withConfig: false });
    const item = [...container!.querySelectorAll(".brink-context-menu-item")].find((e) =>
      e.textContent?.includes("in this project"),
    );
    expect(item?.getAttribute("aria-disabled")).toBe("true");
  });
});

describe("what each item writes", () => {
  it("this line — a directive above the offending line", () => {
    const { files } = mount(diag());
    click("Suppress E035 on this line");
    expect(files.get("main.ink")).toBe(`=== intro ===
// brink-disable E035
VAR roll = 0
`);
  });

  it("this file — names the code, so only that code is silenced", () => {
    const { files } = mount(diag());
    click("Suppress E035 in this file");
    expect(files.get("main.ink")).toBe(`// brink-disable-file E035\n${STORY}`);
  });

  it("all in this file — the -all spelling, not the bare form", () => {
    // Bare `// brink-disable-file` is now reported as E192; the blanket
    // meaning moved to `-all`.
    const { files } = mount(diag());
    click("Suppress all diagnostics in this file");
    expect(files.get("main.ink")).toBe(`// brink-disable-file-all\n${STORY}`);
  });

  it("this project — writes to brink.toml, not the story", () => {
    const { files, applied } = mount(diag());
    click("Suppress E035 in this project");
    expect(files.get("brink.toml")).toContain('E035 = "allow"');
    expect(files.get("main.ink")).toBe(STORY);
    expect(applied.map(([p]) => p)).toEqual(["brink.toml"]);
  });

  it("a disabled item writes nothing", () => {
    const { applied } = mount(diag(), { withConfig: false });
    click("Suppress E035 in this project");
    expect(applied).toEqual([]);
  });
});
