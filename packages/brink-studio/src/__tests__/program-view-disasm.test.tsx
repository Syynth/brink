/**
 * The Program Explorer's Disassembly view (#3339 phase 3).
 *
 * The claims worth pinning: every operand RESOLVES (emit_line to its line
 * text with a working jump into the Line tables view; globals to live
 * values while paused; call_external to its binding contract; jumps to
 * their landing offset); labeled containers keep their NAMES in the rail
 * (`enter_container barter.opts` finds `opts`, never a hand-joined c-N);
 * and the provenance column rides the same SourceLink contract as the
 * Line tables view — converted spans, line-numbered labels.
 */
import { describe, expect, it, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  EDITOR_REVEAL_COMMAND_ID,
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

// "x — y\n…" — the em-dash keeps the byte→UTF-16 conversion honest here too.
const SRC = "x — y\nBarter opens.\n";

const MODEL: ProgramModel = {
  checksum: "0xabc",
  debug_info: true,
  globals: [],
  lists: [],
  externals: [
    { name: "play_se", arg_count: 1, fallback: "play_se_fallback" },
    { name: "teleport", arg_count: 3 },
  ],
  knots: [
    {
      path: "barter",
      name: "barter",
      kind: "knot",
      flags: [],
      path_hash: 1,
      container_idx: 1,
      byte_size: 40,
      container_count: 3,
      anon: [
        {
          label: "opts",
          container_idx: 2,
          byte_size: 12,
          disasm: [{ offset: 0, text: "emit_line #1 0" }],
        },
        { label: "c-0", container_idx: 3, byte_size: 0, disasm: [] },
      ],
      disasm: [
        {
          offset: 0,
          text: "emit_line #0 0",
          src: { file: "scenes/barter.ink", start: 8, end: 21 },
        },
        { offset: 4, text: "get_global gold" },
        { offset: 7, text: "jump_if_false 18" },
        { offset: 10, text: "call_external play_se argc=1" },
        { offset: 14, text: "call_external teleport argc=3" },
      ],
      children: [],
    },
  ],
};

const LINES: LinesTable = {
  version: 1,
  source_checksum: "0xabc",
  scopes: [
    {
      name: "barter",
      id: "0x01",
      lines: [
        { index: 0, content: "Barter opens.", hash: "a" },
        { index: 1, content: "Take it or leave it.", hash: "b" },
      ],
    },
  ] as LinesTable["scopes"],
};

function mount(withDebugState = false): { dispatched: [string, unknown][]; store: StudioStore } {
  const store = createStudioStore();
  store.setState({ programModel: MODEL, programLines: LINES, entryFile: "main.ink" });
  store.setState({
    _project: {
      getSession: () => ({
        getFileSource: (path: string) => (path === "scenes/barter.ink" ? SRC : null),
      }),
    } as never,
  });
  if (withDebugState) {
    store.setState({
      programChecksum: "0xabc",
      compiledChecksum: "0xabc",
      debugState: {
        status: "active",
        position: { container_idx: 1, offset: 4 },
        turn_index: 0,
        globals: [{ name: "gold", value: "12" }],
        call_stack: [],
        visit_counts: [],
      } as never,
    });
  }
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const commands = new CommandRegistry();
  const dispatched: [string, unknown][] = [];
  commands.register({
    id: EDITOR_REVEAL_COMMAND_ID,
    title: "Editor: Reveal Location",
    run: (arg) => {
      dispatched.push([EDITOR_REVEAL_COMMAND_ID, arg]);
    },
  });
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
  act(() => {
    [...container!.querySelectorAll<HTMLButtonElement>(".pv-seg-item")]
      .find((b) => b.textContent === "Disassembly")!
      .click();
  });
  return { dispatched, store };
}

describe("the Disassembly view", () => {
  it("lists labeled containers by NAME in the rail, c-N only for the unnamed", () => {
    mount();
    const names = [...container!.querySelectorAll(".pv-lines-scope-name")].map(
      (n) => n.textContent,
    );
    expect(names).toEqual(["barter", "opts", "c-0"]);
  });

  it("ghosts emit_line with its text and jumps into the Line tables view", () => {
    mount();
    expect(container!.querySelector(".pv-disasm-res-line")?.textContent).toContain(
      "Barter opens.",
    );
    act(() => {
      [...container!.querySelectorAll<HTMLButtonElement>("button")]
        .find((b) => b.textContent === "line ›")!
        .click();
    });
    // Landed in the Line tables view with the row marked.
    expect(container!.querySelector(".pv-lines-head-name")?.textContent).toBe("barter");
    expect(container!.querySelector(".pv-lines-row-target")).not.toBeNull();
  });

  it("shows a paused global's live value, and nothing while not paused", () => {
    mount(false);
    expect(container!.textContent).not.toContain("= 12 now");
    act(() => root?.unmount());
    container?.remove();
    mount(true);
    expect(container!.textContent).toContain("= 12 now");
  });

  it("names a jump's landing offset and an external's binding contract", () => {
    mount();
    expect(container!.textContent).toContain("→ +0x12"); // 18 → hex
    expect(container!.textContent).toContain("fallback body if unbound");
    expect(container!.textContent).toContain("host binding required");
  });

  it("provenance column: line-numbered label, converted span, one contract", () => {
    const { dispatched } = mount();
    const link = [...container!.querySelectorAll<HTMLButtonElement>(".pv-disasm-src .pv-lines-source-link")];
    expect(link).toHaveLength(1); // only the row that HAS provenance
    expect(link[0].textContent).toBe("barter.ink:2");
    act(() => link[0].click());
    // Bytes {8,21} cross the em-dash (3 bytes / 1 unit): units {6,19}.
    expect(dispatched).toEqual([
      [
        EDITOR_REVEAL_COMMAND_ID,
        { kind: "source", file: "scenes/barter.ink", span: { start: 6, end: 19 } },
      ],
    ]);
  });

  it("says when provenance is off rather than showing bare rows", () => {
    const { store } = mount();
    act(() => {
      store.setState({ programModel: { ...MODEL, debug_info: false } });
    });
    expect(container!.textContent).toContain("no debug info — provenance off");
  });

  it("renders an empty container honestly", () => {
    mount();
    act(() => {
      [...container!.querySelectorAll<HTMLButtonElement>(".pv-lines-scope")]
        .find((b) => b.textContent?.includes("c-0"))!
        .click();
    });
    expect(container!.textContent).toContain("empty container");
  });
});
