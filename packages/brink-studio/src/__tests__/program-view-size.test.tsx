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

const groupLabels = (): string[] =>
  [...container!.querySelectorAll(".pv-size-group-label")].map((l) => l.textContent ?? "");

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
    expect(groupLabels().sort()).toEqual(
      ["bytecode", "debug info", "definitions & tables", "line tables"].sort(),
    );
    const debug = [...container!.querySelectorAll(".pv-size-group")].find((b) =>
      b.textContent?.includes("debug info"),
    );
    expect(debug?.classList.contains("pv-size-block-debug")).toBe(true);
  });

  it("nests children inside their groups, with the framing remainder shown as encoding", () => {
    mount();
    // Every block is mounted at the top level — the zoom is only a CSS
    // transition — and the section-vs-content gap is an explicit block.
    const bytecode = [...container!.querySelectorAll(".pv-size-group")].find((g) =>
      g.textContent?.includes("bytecode"),
    )!;
    const titles = [...bytecode.querySelectorAll(".pv-size-block")].map(
      (b) => b.getAttribute("title") ?? "",
    );
    expect(titles.some((t) => t.startsWith("big"))).toBe(true);
    expect(titles.some((t) => t.startsWith("small"))).toBe(true);
    // 4200 section − 4000 knot content = 200 B of real framing.
    const encoding = bytecode.querySelector(".pv-size-block-encoding");
    expect(encoding?.getAttribute("title")).toContain("200 B");
  });

  it("every child carries its full facts in the tooltip, size regardless", () => {
    // Child TEXT is size-gated by a container query (labels appear only at
    // readable sizes — a stylesheet behavior jsdom cannot evaluate, so the
    // gating itself is verified live); the tooltip is the size-independent
    // carrier this pins.
    mount();
    for (const block of container!.querySelectorAll(".pv-size-block")) {
      expect(block.getAttribute("title")).toMatch(/ — .+ · /);
    }
  });

  it("clicking a CHILD zooms its group — no aiming for the header strip", () => {
    mount();
    const bytecode = [...container!.querySelectorAll(".pv-size-group")].find((g) =>
      g.textContent?.includes("bytecode"),
    ) as HTMLElement;
    act(() => {
      bytecode.querySelector<HTMLButtonElement>(".pv-size-block")!.click();
    });
    expect(bytecode.classList.contains("pv-size-zoomed")).toBe(true);
  });

  it("shipping-only removes the debug block and re-flows the total", () => {
    mount();
    act(() => {
      [...container!.querySelectorAll<HTMLButtonElement>(".pv-seg-item")]
        .find((b) => b.textContent === "shipping only")!
        .click();
    });
    expect(groupLabels()).not.toContain("debug info");
    expect(container!.querySelector(".pv-lines-head")!.textContent).toContain("8.0 KB");
  });

  it("zooms by transition: the group fills the container, breadcrumb back", () => {
    mount();
    const head = [...container!.querySelectorAll<HTMLButtonElement>(".pv-size-group-head")].find(
      (b) => b.textContent?.includes("bytecode"),
    )!;
    act(() => head.click());
    const group = head.closest(".pv-size-group") as HTMLElement;
    expect(group.classList.contains("pv-size-zoomed")).toBe(true);
    expect(group.style.width).toBe("100%");
    // Siblings fade rather than unmount — they are mid-transition targets.
    const dimmed = [...container!.querySelectorAll(".pv-size-dimmed")];
    expect(dimmed.length).toBeGreaterThan(0);
    expect(container!.querySelector(".pv-lines-head")!.textContent).toContain("bytecode");
    act(() => {
      container!.querySelector<HTMLButtonElement>(".pv-size-crumb")!.click();
    });
    expect(group.classList.contains("pv-size-zoomed")).toBe(false);
    expect(container!.querySelectorAll(".pv-size-dimmed")).toHaveLength(0);
  });

  it("a debug-free compile says this IS the shipping size, no dead toggle", () => {
    mount({ ...REPORT, debug: 0, total: 8192, sections: REPORT.sections.filter((s) => s.kind !== "DebugInfo") });
    const head = container!.querySelector(".pv-lines-head")!.textContent!;
    expect(head).toContain("this IS the shipping size");
    expect(head).not.toContain("with debug info");
  });
});
