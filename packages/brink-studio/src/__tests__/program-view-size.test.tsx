/**
 * The Program Explorer's Size view (#3339 phase 4).
 *
 * The honesty claims: the totals line states the real split; "shipping
 * only" removes the debug block and re-flows against the exact shipping
 * size; a compile without debug info says this IS the shipping size
 * rather than offering a toggle that changes nothing; zooming shows a
 * group's own children with a working breadcrumb back.
 */
import { describe, expect, it, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
} from "@brink/studio-shell";
import { createStudioStore } from "@brink/studio-store";
import { ProgramView, StoreProvider } from "@brink/studio-ui";
import type { ProgramModel, SizeReport } from "@brink/wasm-types";

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

const MODEL: ProgramModel = {
  checksum: "0xabc",
  debug_info: true,
  globals: [],
  lists: [],
  externals: [],
  knots: [
    {
      path: "big",
      name: "big",
      kind: "knot",
      flags: [],
      path_hash: 1,
      container_idx: 1,
      byte_size: 3000,
      container_count: 1,
      anon: [],
      disasm: [],
      children: [],
    },
    {
      path: "small",
      name: "small",
      kind: "knot",
      flags: [],
      path_hash: 2,
      container_idx: 2,
      byte_size: 1000,
      container_count: 1,
      anon: [],
      disasm: [],
      children: [],
    },
  ],
};

const REPORT: SizeReport = {
  total: 10240,
  shipping: 8192,
  debug: 2048,
  header: 100,
  sections: [
    { kind: "Containers", bytes: 4200 },
    { kind: "LineTables", bytes: 1500 },
    { kind: "DebugInfo", bytes: 2048 },
    { kind: "NameTable", bytes: 1200 },
    { kind: "Variables", bytes: 1192 },
  ],
  line_scopes: [
    { name: "big", bytes: 900 },
    { name: "small", bytes: 600 },
  ],
};

function mount(report: SizeReport = REPORT) {
  const store = createStudioStore();
  store.setState({ programModel: MODEL, programSize: report, entryFile: "main.ink" });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        {
          commands: new CommandRegistry(),
          themes: new ThemeService(),
          keymapOverrides: new KeymapOverridesService(),
          isMac: true,
        } as never,
        createElement(StoreProvider, { store } as never, createElement(ProgramView)),
      ),
    );
  });
  act(() => {
    [...container!.querySelectorAll<HTMLButtonElement>(".pv-seg-item")]
      .find((b) => b.textContent === "Size")!
      .click();
  });
}

const blockLabels = (): string[] =>
  [...container!.querySelectorAll(".pv-size-block-label")].map((l) => l.textContent ?? "");

describe("the Size view", () => {
  it("states the real split in the totals line", () => {
    mount();
    const head = container!.querySelector(".pv-lines-head")!.textContent!;
    expect(head).toContain("10.0 KB");
    expect(head).toContain("8.0 KB shipping");
    expect(head).toContain("2.0 KB debug info (20%)");
  });

  it("maps the top level: bytecode, line tables, debug (dashed), definitions", () => {
    mount();
    expect(blockLabels().sort()).toEqual(
      ["bytecode", "debug info", "definitions & tables", "line tables"].sort(),
    );
    const debug = [...container!.querySelectorAll(".pv-size-block")].find((b) =>
      b.textContent?.includes("debug info"),
    );
    expect(debug?.classList.contains("pv-size-block-debug")).toBe(true);
  });

  it("shipping-only removes the debug block and re-flows the total", () => {
    mount();
    act(() => {
      [...container!.querySelectorAll<HTMLButtonElement>(".pv-seg-item")]
        .find((b) => b.textContent === "shipping only")!
        .click();
    });
    expect(blockLabels()).not.toContain("debug info");
    expect(container!.querySelector(".pv-lines-head")!.textContent).toContain("8.0 KB");
  });

  it("zooms into bytecode by knot, breadcrumb back", () => {
    mount();
    act(() => {
      [...container!.querySelectorAll<HTMLButtonElement>(".pv-size-block")]
        .find((b) => b.textContent?.includes("bytecode"))!
        .click();
    });
    expect(blockLabels().sort()).toEqual(["big", "small"]);
    act(() => {
      container!.querySelector<HTMLButtonElement>(".pv-size-crumb")!.click();
    });
    expect(blockLabels()).toContain("bytecode");
  });

  it("a debug-free compile says this IS the shipping size, no dead toggle", () => {
    mount({ ...REPORT, debug: 0, total: 8192, sections: REPORT.sections.filter((s) => s.kind !== "DebugInfo") });
    const head = container!.querySelector(".pv-lines-head")!.textContent!;
    expect(head).toContain("this IS the shipping size");
    expect(head).not.toContain("with debug info");
  });
});
