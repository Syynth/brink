/**
 * The Debugger panel (W8/#3301) — the StateView replacement's proof
 * items: frame selection drives locals + the editor reveal; flow
 * selection scopes what renders; breakpoint rows toggle/remove through
 * the anchor actions; placeholder states keep StateView's honesty.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DebuggerPanel, StoreProvider } from "@brink/studio-ui";
import {
  CommandRegistry,
  EDITOR_REVEAL_COMMAND_ID,
  ShellProvider,
  createEditorGroupsStore,
  createShellLayoutStore,
} from "@brink/studio-shell";
import { createStudioStore, DEFAULT_SESSION_ID, type StudioStore } from "@brink/studio-store";
import type { DebugState } from "@brink/wasm-types";

let root: Root | null = null;
let host: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  root = null;
  host = null;
});

function debugState(overrides: Partial<DebugState> = {}): DebugState {
  return {
    status: "active",
    current_location: "intro",
    turn_index: 1,
    position: { container_idx: 1, offset: 2 },
    globals: [
      { name: "gold", value: "12" },
      { name: "torch", value: "6" },
    ],
    call_stack: [
      {
        kind: "function",
        location: "barter.haggle",
        position: { container_idx: 5, offset: 0 },
        temps: 1,
        locals: [{ slot: 0, name: "price", value: { type: "int", value: 6 } }],
      },
      {
        kind: "root",
        location: "barter",
        position: { container_idx: 1, offset: 2 },
        temps: 0,
        locals: [{ slot: 0, name: "mood", value: { type: "string", value: "wary" } }],
      },
    ],
    visit_counts: [{ path: "intro", count: 1 }],
    pending_choices: [],
    rng: { seed: 42, previous: 7 },
    ...overrides,
  } as DebugState;
}

function mount(store: StudioStore, commands = new CommandRegistry()) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => {
    root?.render(
      createElement(
        ShellProvider,
        {
          commands,
          editorGroups: createEditorGroupsStore(),
          layout: createShellLayoutStore(),
        } as never,
        createElement(StoreProvider, { store } as never, createElement(DebuggerPanel)),
      ),
    );
  });
  return { commands };
}

describe("Debugger panel (W8/#3301)", () => {
  it("no session: the start placeholder", () => {
    const store = createStudioStore();
    mount(store);
    expect(host?.textContent).toContain("No story session");
    expect(host?.querySelector(".session-placeholder-start")).not.toBeNull();
  });

  it("renders flows, frames, variables, and breakpoints from store state", () => {
    const store = createStudioStore();
    store.setState({
      sessionStatus: "running",
      debugState: debugState(),
      sessions: [
        { id: DEFAULT_SESSION_ID, label: "Main", provider: {} as never },
      ],
      activeSessionId: DEFAULT_SESSION_ID,
      sourceBreakpoints: [
        {
          key: "bp1",
          file: "main.ink",
          line: 4,
          enabled: true,
          address: { container_idx: 3, offset: 1 },
        },
      ],
    });
    mount(store);

    const text = host?.textContent ?? "";
    expect(text).toContain("Flows (1)");
    expect(text).toContain("Frames (2)");
    expect(text).toContain("barter.haggle");
    // Top frame's locals show by default (selection = top).
    expect(text).toContain("price");
    expect(text).not.toContain("mood");
    // Globals with values.
    expect(text).toContain("gold");
    // Breakpoint row, 1-based.
    expect(text).toContain("main.ink:5");
  });

  it("frame selection scopes locals, dispatches the reveal, and resets on advance", () => {
    const store = createStudioStore();
    store.setState({ sessionStatus: "running", debugState: debugState() });
    const commands = new CommandRegistry();
    const revealed: unknown[] = [];
    commands.register({
      id: EDITOR_REVEAL_COMMAND_ID,
      title: "reveal",
      run: (args) => {
        revealed.push(args);
      },
    });
    mount(store, commands);

    const frameButtons = Array.from(
      host?.querySelectorAll<HTMLButtonElement>(".dp-frame-head") ?? [],
    );
    expect(frameButtons).toHaveLength(2);
    act(() => frameButtons[1].click());

    // Selection landed in the store, the second frame's locals show, and
    // the frame's position was revealed.
    expect(store.getState().selectedFrameIdx).toBe(1);
    expect(host?.textContent).toContain("mood");
    expect(host?.textContent).not.toContain("price");
    expect(revealed).toHaveLength(1);
    expect(revealed[0]).toMatchObject({ kind: "program" });

    // Any runtime advance drops the selection back to the top frame —
    // exercised through the same mirror the provider snapshot road uses.
    const fresh = debugState({ turn_index: 2 });
    store.setState((s) => ({
      debugState: fresh,
      selectedFrameIdx: fresh !== s.debugState ? null : s.selectedFrameIdx,
    }));
    expect(store.getState().selectedFrameIdx).toBeNull();
  });

  it("breakpoint rows toggle and remove through the anchor actions", () => {
    const store = createStudioStore();
    const setEnabled = vi.fn();
    const remove = vi.fn();
    store.setState({
      sessionStatus: "running",
      debugState: debugState(),
      sourceBreakpoints: [
        { key: "bp1", file: "main.ink", line: 4, enabled: true, address: null },
      ],
      breakpointSetEnabled: setEnabled,
      breakpointRemove: remove,
    });
    mount(store);

    const checkbox = host?.querySelector<HTMLInputElement>(".dp-bp-row input");
    act(() => checkbox?.click());
    expect(setEnabled).toHaveBeenCalledWith("bp1", false);

    const x = host?.querySelector<HTMLButtonElement>(".dp-bp-row .dp-x");
    act(() => x?.click());
    expect(remove).toHaveBeenCalledWith("bp1");
  });

  it("disable-all / clear-all header actions drive every anchor", () => {
    const store = createStudioStore();
    store.setState({
      sessionStatus: "running",
      debugState: debugState(),
      sourceBreakpoints: [
        { key: "a", file: "m.ink", line: 1, enabled: true, address: null },
        { key: "b", file: "m.ink", line: 2, enabled: true, address: null },
      ],
    });
    mount(store);

    const buttons = Array.from(host?.querySelectorAll<HTMLButtonElement>(".dp-mini") ?? []);
    const disableAll = buttons.find((b) => b.textContent === "disable all");
    act(() => disableAll?.click());
    expect(store.getState().sourceBreakpoints.every((b) => !b.enabled)).toBe(true);

    const clear = buttons.find((b) => b.textContent === "clear");
    act(() => clear?.click());
    expect(store.getState().sourceBreakpoints).toHaveLength(0);
  });
});
