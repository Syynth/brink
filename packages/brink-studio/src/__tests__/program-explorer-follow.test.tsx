/**
 * Program Explorer frame-follow + reveal target (W9/#3302): the
 * current-instruction highlight follows the SELECTED stack frame (not
 * just the top), degraded still suppresses, and a reveal target marks
 * its instruction row.
 */
import { describe, expect, it, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ProgramView, StoreProvider } from "@brink/studio-ui";
import {
  CommandRegistry,
  ShellProvider,
  createEditorGroupsStore,
  createShellLayoutStore,
} from "@brink/studio-shell";
import { createStudioStore, type StudioStore } from "@brink/studio-store";

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

const MODEL = {
  checksum: "0xabc",
  globals: [],
  lists: [],
  externals: [],
  knots: [
    {
      path: "top",
      name: "top",
      kind: "knot",
      flags: [],
      container_idx: 5,
      path_hash: 0,
      disasm: [
        { offset: 1, text: "emit_line #0" },
        { offset: 2, text: "emit_line #1" },
      ],
      children: [],
    },
  ],
} as never;

function mount(store: StudioStore) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => {
    root?.render(
      createElement(
        ShellProvider,
        {
          commands: new CommandRegistry(),
          editorGroups: createEditorGroupsStore(),
          layout: createShellLayoutStore(),
        } as never,
        createElement(StoreProvider, { store } as never, createElement(ProgramView)),
      ),
    );
  });
}

function openKnot() {
  const header = host?.querySelector<HTMLButtonElement>(".pv-knot-header");
  act(() => header?.click());
}

function currentOffset(): string | null {
  return (
    host?.querySelector(".pv-current-instruction .pv-disasm-offset")?.textContent ?? null
  );
}

describe("Program Explorer frame-follow (W9/#3302)", () => {
  it("no selection: the TOP position highlights; a selected frame retargets it", () => {
    const store = createStudioStore();
    store.setState({
      programModel: MODEL,
      programChecksum: "0xabc",
      compiledChecksum: "0xabc",
      debugState: {
        position: { container_idx: 5, offset: 1 },
        call_stack: [
          { kind: "function", temps: 0, position: { container_idx: 5, offset: 1 } },
          { kind: "root", temps: 0, position: { container_idx: 5, offset: 2 } },
        ],
      } as never,
    });
    mount(store);
    openKnot();
    expect(currentOffset()).toBe("1");

    act(() => store.getState().selectFrame(1));
    expect(currentOffset()).toBe("2");
  });

  it("degraded suppresses the highlight even with a frame selected", () => {
    const store = createStudioStore();
    store.setState({
      programModel: MODEL,
      programChecksum: "0xOLD",
      compiledChecksum: "0xabc",
      selectedFrameIdx: 1,
      debugState: {
        position: { container_idx: 5, offset: 1 },
        call_stack: [
          { kind: "function", temps: 0, position: { container_idx: 5, offset: 1 } },
          { kind: "root", temps: 0, position: { container_idx: 5, offset: 2 } },
        ],
      } as never,
    });
    mount(store);
    openKnot();
    expect(currentOffset()).toBeNull();
  });

  it("a reveal target auto-opens the knot and marks its instruction", () => {
    const store = createStudioStore();
    store.setState({
      programModel: MODEL,
      programChecksum: "0xabc",
      compiledChecksum: "0xabc",
      programExplorerTarget: { address: { container_idx: 5, offset: 2 }, nonce: 1 },
    });
    mount(store);
    // NOT manually opened — the target opens the row itself.
    const marked = host?.querySelector(".pv-target-instruction .pv-disasm-offset");
    expect(marked?.textContent).toBe("2");
  });
});
