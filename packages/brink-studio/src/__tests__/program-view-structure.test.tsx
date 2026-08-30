/**
 * The Program Explorer's Structure view (#3339 shell + first view).
 *
 * The redesign's claims worth pinning: the program reads as a NAMED thing
 * (header identity from the entry file, not a bare hex string); knot rows
 * carry size at a glance (bytes from the #3342 rollups, lines joined from
 * the compiled lines table); externals state their contract (fallback body
 * vs host-required); and the view switch ships with the unbuilt views as
 * designed slots — disabled, each naming where it is — never as live
 * buttons that do nothing.
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
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import { ProgramView, StoreProvider } from "@brink/studio-ui";
import type { LinesTable, ProgramModel } from "@brink/wasm-types";

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

const MODEL: ProgramModel = {
  checksum: "0x54b500f2",
  globals: [{ name: "gold", ty: "int", default: "12" }],
  lists: [],
  externals: [
    { name: "play_se", arg_count: 1, fallback: "play_se_fallback" },
    { name: "teleport", arg_count: 3 },
  ],
  knots: [
    {
      path: "big",
      name: "big",
      kind: "knot",
      flags: [],
      path_hash: 1,
      container_idx: 1,
      byte_size: 300,
      container_count: 3,
      disasm: [],
      children: [
        {
          path: "big.inner",
          name: "inner",
          kind: "stitch",
          flags: [],
          path_hash: 2,
          container_idx: 2,
          byte_size: 100,
          container_count: 2,
          disasm: [],
          children: [],
        },
      ],
    },
    {
      path: "small",
      name: "small",
      kind: "knot",
      flags: [],
      path_hash: 3,
      container_idx: 3,
      byte_size: 100,
      container_count: 1,
      disasm: [],
      children: [],
    },
  ],
};

const LINES: LinesTable = {
  version: 1,
  source_checksum: "0x54b500f2",
  scopes: [
    {
      name: "big",
      lines: [
        { index: 0, content: "Plain.", hash: "a" },
        { index: 1, content: { template: ["You have ", { slot: 0 }] }, hash: "b" },
      ],
    },
    { name: "big.inner", lines: [{ index: 0, content: "Inner.", hash: "c" }] },
    { name: "small", lines: [{ index: 0, content: "Small.", hash: "d" }] },
  ] as LinesTable["scopes"],
};

function mount(lines: LinesTable | null = LINES): StudioStore {
  const store = createStudioStore();
  store.setState({ programModel: MODEL, programLines: lines, entryFile: "stories/toppled-temple.ink" });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const commands = new CommandRegistry();
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        {
          commands,
          themes: new ThemeService(),
          keymapOverrides: new KeymapOverridesService(),
          isMac: true,
        } as never,
        createElement(StoreProvider, { store } as never, createElement(ProgramView)),
      ),
    );
  });
  return store;
}

describe("the Structure view", () => {
  it("names the program from the entry file, beside the checksum chip", () => {
    mount();
    expect(container!.querySelector(".pv-program-name")?.textContent).toBe("toppled-temple");
    expect(container!.querySelector(".pv-checksum")?.textContent).toBe("0x54b500f2");
    expect(container!.querySelector(".pv-counts")?.textContent).toContain("2 knots");
    expect(container!.querySelector(".pv-counts")?.textContent).toContain("4 lines");
  });

  it("ships the view switch with unbuilt views disabled, not live no-ops", () => {
    mount();
    const items = [...container!.querySelectorAll<HTMLButtonElement>(".pv-seg-item")];
    expect(items.map((b) => b.textContent)).toEqual([
      "Structure",
      "Line tables",
      "Disassembly",
      "Size",
    ]);
    expect(items[0].classList.contains("active")).toBe(true);
    expect(items.slice(1).every((b) => b.disabled)).toBe(true);
    // Each disabled slot says where its view is, so it reads as designed
    // rather than broken.
    expect(items[1].title).toContain("#3339");
  });

  it("sizes knot rows by SUBTREE — a knot's stitches count toward it", () => {
    mount();
    const labels = [...container!.querySelectorAll(".pv-size-label")].map((l) => l.textContent);
    // big = 300 own + 100 stitch = 400 B; its lines = 2 own + 1 stitch = 3.
    expect(labels[0]).toContain("400 B");
    expect(labels[0]).toContain("3 lines");
    expect(labels[1]).toContain("100 B");
    // The container count rolls up the same way.
    const cont = [...container!.querySelectorAll(".pv-size-cont")].map((c) => c.textContent);
    expect(cont[0]).toContain("5");
    expect(cont[1]).toContain("1");
  });

  it("degrades to bytes-only rows when no lines table is loaded", () => {
    // An older compile product without the #3342 capture: bars and byte
    // labels still render; nothing claims a line count of zero.
    mount(null);
    const label = container!.querySelector(".pv-size-label");
    expect(label?.textContent).toContain("400 B");
    expect(label?.textContent).not.toContain("lines");
  });

  it("badges an external by its contract: fallback body vs host-required", () => {
    mount();
    expect(container!.querySelector(".pv-ext-fallback")).not.toBeNull();
    expect(container!.querySelector(".pv-ext-host")).not.toBeNull();
  });

  it("totals the program in the footer, templates counted from the table", () => {
    mount();
    const footer = container!.querySelector(".pv-footer")!.textContent!;
    expect(footer).toContain("500 B");
    expect(footer).toContain("4 lines in 3 tables");
    expect(footer).toContain("1 template");
  });
});
