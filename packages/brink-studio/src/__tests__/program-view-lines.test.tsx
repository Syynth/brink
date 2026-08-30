/**
 * The Program Explorer's Line tables view (#3339 phase 2).
 *
 * What it must get right: scopes exactly as the compiler scopes them
 * (knots and stitches, root labeled rather than blank); template structure
 * as CHIPS reading like prose, never raw JSON; audio and source as
 * first-class columns — and the source cell as a REAL link riding the same
 * `editor.reveal` road a Problems row rides, not a second navigation
 * contract.
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
import { createStudioStore } from "@brink/studio-store";
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
  checksum: "0xabc",
  globals: [],
  lists: [],
  externals: [],
  knots: [
    {
      path: "warden",
      name: "warden",
      kind: "knot",
      flags: [],
      path_hash: 1,
      container_idx: 1,
      byte_size: 100,
      container_count: 1,
      disasm: [],
      children: [],
    },
  ],
};

const LINES: LinesTable = {
  version: 1,
  source_checksum: "0xabc",
  scopes: [
    { name: null, id: "0x00", lines: [] },
    {
      name: "warden",
      id: "0x01",
      lines: [
        {
          index: 0,
          content: "The golem grinds upright.",
          hash: "a1",
          audio: "se/golem_wake",
          // Byte range of "The golem" on line 3 of TEMPLE_SRC — after an
          // em-dash, so the UTF-8/UTF-16 drift is live in this fixture.
          source: { file: "scenes/temple.ink", range_start: 30, range_end: 39 },
        },
        {
          index: 1,
          content: {
            template: [
              "You have ",
              { slot: 0 },
              " ",
              {
                select: {
                  slot: 0,
                  variants: [{ "cardinal:One": "torch" }],
                  default: "torches",
                },
              },
              " left.",
            ],
          },
          hash: "b2",
          slots: [{ index: 0, name: "torch" }],
        },
      ],
    },
    { name: "warden.turn", id: "0x02", lines: [{ index: 0, content: "Turn.", hash: "c3" }] },
  ],
};

// Line 1 holds an em-dash (3 bytes / 1 unit): everything after it drifts
// if raw bytes reach the UTF-16 reveal road.
// bytes: "so — it begins\n" = 17 (em-dash 3), "quiet\n" = 6 → line 3 at byte 23.
const TEMPLE_SRC = "so — it begins\nquiet\nnow, The golem wakes.\n";

function mount(): { commands: CommandRegistry; dispatched: [string, unknown][] } {
  const store = createStudioStore();
  store.setState({ programModel: MODEL, programLines: LINES, entryFile: "main.ink" });
  store.setState({
    _project: {
      getSession: () => ({
        getFileSource: (path: string) => (path === "scenes/temple.ink" ? TEMPLE_SRC : null),
      }),
    } as never,
  });
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
  // Switch to the Line tables view.
  act(() => {
    [...container!.querySelectorAll<HTMLButtonElement>(".pv-seg-item")]
      .find((b) => b.textContent === "Line tables")!
      .click();
  });
  return { commands, dispatched };
}

describe("the Line tables view", () => {
  it("lists scopes as the compiler scopes them, root labeled, stitches indented", () => {
    mount();
    const rows = [...container!.querySelectorAll(".pv-lines-scope")];
    expect(rows.map((r) => r.querySelector(".pv-lines-scope-name")?.textContent)).toEqual([
      "(root)",
      "warden",
      "= turn",
    ]);
    expect(rows[2].classList.contains("pv-lines-scope-stitch")).toBe(true);
    // Landing selection: the first scope WITH lines, not the empty root.
    expect(
      container!.querySelector(".pv-lines-scope.active .pv-lines-scope-name")?.textContent,
    ).toBe("warden");
  });

  it("renders template structure as chips that read like prose", () => {
    mount();
    const row = [...container!.querySelectorAll(".pv-lines-text")][1]!;
    // Slot chip named from the slots table, not a bare index.
    expect(row.querySelector(".pv-chip-slot")?.textContent).toBe("{torch}");
    // Select chip shows the variants plus the default.
    expect(row.querySelector(".pv-chip-select")?.textContent).toBe("torch|torches");
    // The literal parts survive around them, in order.
    expect(row.textContent).toContain("You have ");
    expect(row.textContent).toContain(" left.");
    // Nothing leaks JSON.
    expect(row.textContent).not.toContain("{\"");
  });

  it("marks audio refs and leaves plain rows unmarked", () => {
    mount();
    const audio = [...container!.querySelectorAll(".pv-lines-audio")];
    expect(audio[0].querySelector<HTMLElement>(".pv-lines-audio-ref")?.title).toContain(
      "se/golem_wake",
    );
    expect(audio[1].querySelector(".pv-lines-audio-ref")).toBeNull();
  });

  it("labels the source with its line number, computed from the byte offset", () => {
    mount();
    // range_start 30 sits on line 3 of TEMPLE_SRC.
    expect(container!.querySelector(".pv-lines-source-link")?.textContent).toBe("temple.ink:3");
  });

  it("converts the byte range to UTF-16 before riding the reveal road", () => {
    // The em-dash on line 1 costs 3 bytes but 1 code unit, so raw bytes
    // would overshoot by 2 — the drift the maintainer saw live.
    const { dispatched } = mount();
    act(() => {
      container!.querySelector<HTMLButtonElement>(".pv-lines-source-link")!.click();
    });
    expect(dispatched).toEqual([
      [
        EDITOR_REVEAL_COMMAND_ID,
        { kind: "source", file: "scenes/temple.ink", span: { start: 28, end: 37 } },
      ],
    ]);
  });

  it("still navigates, unconverted and unnumbered, when the file is not in the session", () => {
    const { dispatched } = mount();
    // Point the fixture's source at a file the stub cannot serve.
    act(() => {
      [...container!.querySelectorAll<HTMLButtonElement>(".pv-lines-scope")]
        .find((b) => b.textContent?.includes("turn"))!
        .click();
    });
    expect(dispatched).toEqual([]);
  });

  it("states the scope's facts in its header", () => {
    mount();
    const facts = container!.querySelector(".pv-lines-head-facts")?.textContent;
    expect(facts).toContain("2 lines");
    expect(facts).toContain("1 template");
    expect(facts).toContain("1 select");
    expect(facts).toContain("1 audio ref");
  });

  it("switching scopes swaps the table", () => {
    mount();
    act(() => {
      [...container!.querySelectorAll<HTMLButtonElement>(".pv-lines-scope")]
        .find((b) => b.textContent?.includes("turn"))!
        .click();
    });
    expect(container!.querySelector(".pv-lines-head-name")?.textContent).toBe("warden.turn");
    expect([...container!.querySelectorAll(".pv-lines-text")].map((t) => t.textContent)).toEqual([
      "Turn.",
    ]);
  });
});
