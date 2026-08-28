/**
 * State View frame locals (#3140, consuming D7/#3185).
 *
 * The runtime, the wasm bridge and the TS types already carried named
 * locals; nothing in the studio read them. These tests cover the two things
 * that would rot silently:
 *
 * 1. **The tri-state.** `locals` is `undefined` (no debug info), `[]`
 *    (debug info, genuinely none) or populated. Collapsing the first two
 *    into a blank panel tells an author "this function has no locals" when
 *    the truth is "this build cannot tell you" — a wrong statement, not a
 *    missing one.
 * 2. **Structure survives rendering.** D7 made locals a tagged union
 *    precisely so a list is distinguishable from a string that reads like
 *    one. Flattening it back to a display string in the view would undo
 *    that silently, and still look fine.
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
import { StateView, StoreProvider } from "@brink/studio-ui";
import type { DebugFrame, DebugState } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let container: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function debugState(frames: DebugFrame[]): DebugState {
  return {
    status: "active",
    current_location: "intro",
    turn_index: 0,
    globals: [],
    call_stack: frames,
    visit_counts: [],
    pending_choices: [],
    // Present because the view reads it unconditionally — a fixture missing
    // it fails inside StateView rather than in the assertion, which reads
    // like a bug in the code under test.
    rng: { seed: 1, calls: 0 },
  } as unknown as DebugState;
}

function mount(store: StudioStore) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const commands = new CommandRegistry();
  const themes = new ThemeService();
  const overrides = new KeymapOverridesService();
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        { commands, themes, keymapOverrides: overrides, isMac: true } as never,
        createElement(StoreProvider, { store } as never, createElement(StateView)),
      ),
    );
  });
}

function withFrames(frames: DebugFrame[]): StudioStore {
  const store = createStudioStore();
  store.setState({ sessionStatus: "running", debugState: debugState(frames) } as never);
  return store;
}

const frame = (over: Partial<DebugFrame>): DebugFrame =>
  ({ kind: "function", location: "f", temps: 0, ...over }) as DebugFrame;

describe("State View frame locals", () => {
  it("renders each local's name and value inside its own frame", () => {
    const store = withFrames([
      frame({
        location: "haggle",
        temps: 2,
        locals: [
          { slot: 0, name: "offer", value: { type: "int", value: 5 } },
          { slot: 1, name: "seller", value: { type: "string", value: "VENDOR" } },
        ],
      }),
    ]);
    mount(store);

    const rows = [...container!.querySelectorAll(".sv-locals tr")];
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain("offer");
    expect(rows[0].textContent).toContain("5");
    expect(rows[1].textContent).toContain("seller");
    // Quoted, so an empty string is distinguishable from a missing value.
    expect(rows[1].textContent).toContain("“VENDOR”");
  });

  it("keeps two frames' same-named locals apart", () => {
    // The reason locals render INSIDE a frame rather than in one flat
    // section: `gold` in the caller and `gold` in the callee are different
    // variables, and a flat list would have to invent a disambiguation.
    const store = withFrames([
      frame({ location: "outer", temps: 1, locals: [{ slot: 0, name: "gold", value: { type: "int", value: 1 } }] }),
      frame({ location: "inner", temps: 1, locals: [{ slot: 0, name: "gold", value: { type: "int", value: 99 } }] }),
    ]);
    mount(store);

    const tables = [...container!.querySelectorAll(".sv-locals")];
    expect(tables).toHaveLength(2);
    expect(tables[0].textContent).toContain("1");
    expect(tables[1].textContent).toContain("99");
  });

  it("says so when a frame has no debug info, rather than showing nothing", () => {
    // `undefined` means the program carries no DebugInfo — a release
    // export, or a pre-D6 build. Blank would read as "no locals", which is
    // a different and wrong claim.
    const store = withFrames([frame({ location: "haggle", temps: 3, locals: undefined })]);
    mount(store);

    expect(container!.querySelector(".sv-locals")).toBeNull();
    expect(container!.querySelector(".sv-locals-none")?.textContent).toContain("no debug info");
  });

  it("stays quiet for a frame with debug info and genuinely no locals", () => {
    // `[]` is the other half of the tri-state: the build CAN tell us, and
    // the answer is none. Nothing to say.
    const store = withFrames([frame({ location: "haggle", temps: 0, locals: [] })]);
    mount(store);

    expect(container!.querySelector(".sv-locals")).toBeNull();
    expect(container!.querySelector(".sv-locals-none")).toBeNull();
  });

  it("stays quiet for a positionless frame, which never has locals to resolve", () => {
    // An `external` frame carries no bytecode position by construction, so
    // `locals` is absent for a structural reason rather than a build one.
    // Announcing "no debug info" on every one of them would be noise.
    const store = withFrames([frame({ kind: "external", location: undefined, temps: 0, locals: undefined })]);
    mount(store);

    expect(container!.querySelector(".sv-locals-none")).toBeNull();
  });

  it("renders a list's members rather than flattening to a display string", () => {
    // The whole point of D7's structured value: telling "a list with these
    // members" from "a string that reads like a list".
    const store = withFrames([
      frame({
        temps: 1,
        locals: [{ slot: 0, name: "flags", value: { type: "list", members: ["red", "blue"] } }],
      }),
    ]);
    mount(store);

    const members = [...container!.querySelectorAll(".sv-v-member")].map((m) => m.textContent);
    expect(members).toEqual(["red", "blue"]);
  });

  it("distinguishes an empty list from a null", () => {
    // ink's list semantics make "empty" a meaningful state, not an absence.
    const store = withFrames([
      frame({
        temps: 2,
        locals: [
          { slot: 0, name: "empty", value: { type: "list", members: [] } },
          { slot: 1, name: "missing", value: { type: "null" } },
        ],
      }),
    ]);
    mount(store);

    const rows = [...container!.querySelectorAll(".sv-locals tr")].map((r) => r.textContent);
    expect(rows[0]).toContain("(empty list)");
    expect(rows[1]).toContain("null");
  });

  it("expands a struct's fields one level", () => {
    const store = withFrames([
      frame({
        temps: 1,
        locals: [
          {
            slot: 0,
            name: "cue",
            value: {
              type: "struct",
              name: "Cue",
              fields: [{ name: "speaker", value: { type: "string", value: "KID" } }],
            },
          },
        ],
      }),
    ]);
    mount(store);

    const cell = container!.querySelector(".sv-locals .sv-val");
    expect(cell?.textContent).toContain("Cue");
    expect(cell?.textContent).toContain("speaker");
    expect(cell?.textContent).toContain("“KID”");
  });

  it("prints a handle id verbatim, never as a number", () => {
    // `id` is a decimal STRING on the wire because a full-range host token
    // would lose precision as a JS number. Parsing it here would reintroduce
    // exactly that.
    const big = "9007199254740993"; // 2^53 + 1
    const store = withFrames([
      frame({ temps: 1, locals: [{ slot: 0, name: "h", value: { type: "handle", kind: "actor", id: big } }] }),
    ]);
    mount(store);

    expect(container!.querySelector(".sv-v-handle")?.textContent).toContain(big);
  });

  it("keeps the bare temp count, which is right even with no debug info", () => {
    // D7 added `locals` ALONGSIDE `temps` rather than replacing it, and the
    // count is the one number a release build can still report.
    const store = withFrames([frame({ temps: 4, locals: undefined })]);
    mount(store);

    expect(container!.textContent).toContain("4 temps");
  });
});
