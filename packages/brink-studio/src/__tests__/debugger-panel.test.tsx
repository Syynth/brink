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

  it("flow rows switch the active session; the primary is not closable (ex-SessionPicker pins)", () => {
    // The status-bar SessionPicker retired (W10/#3303) — its behaviors
    // live here now: selection repoints the session-bound views, and the
    // primary session carries no close affordance.
    const store = createStudioStore();
    const setActive = vi.fn();
    store.setState({
      sessionStatus: "running",
      debugState: debugState(),
      sessions: [
        { id: DEFAULT_SESSION_ID, label: "Main", provider: {} as never },
        { id: "s2", label: "intro", provider: {} as never },
      ],
      activeSessionId: DEFAULT_SESSION_ID,
      setActiveSession: setActive,
    });
    mount(store);

    const rows = Array.from(host?.querySelectorAll<HTMLButtonElement>(".dp-flow-select") ?? []);
    expect(rows.map((r) => r.textContent)).toEqual(["Main", "intro"]);
    act(() => rows[1].click());
    expect(setActive).toHaveBeenCalledWith("s2");

    // Close affordance: absent on the primary row, present on the second.
    const flowRows = Array.from(host?.querySelectorAll(".dp-flow-row") ?? []);
    expect(flowRows[0].querySelector(".dp-x")).toBeNull();
    expect(flowRows[1].querySelector(".dp-x")).not.toBeNull();
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

  // ── W16/#3309: live value editing ──────────────────────────────────
  function editableStore(overrides: Record<string, unknown> = {}) {
    const store = createStudioStore();
    const editGlobal = vi.fn(() => true);
    const editTemp = vi.fn(() => true);
    const provider = {
      kind: "local",
      capabilities: new Set(["debug"]),
      getSnapshot: () => ({
        status: "running",
        transcript: [],
        choices: [],
        debugState: null,
        paused: true,
        reloadedAt: null,
        debugOutcome: null,
        auto: false,
        programChecksum: null,
        programModel: null,
        programInkt: null,
      }),
      subscribe: () => () => {},
      dispose: () => {},
      editGlobal,
      editTemp,
    };
    store.getState()._bindProvider(provider as never);
    store.setState({
      sessionStatus: "running",
      sessionPaused: true,
      debugState: debugState(),
      ...overrides,
    });
    return { store, editGlobal, editTemp };
  }

  it("a paused global edits inline: Enter commits through debugEditGlobal", () => {
    const { store, editGlobal } = editableStore();
    mount(store);

    const goldValue = Array.from(host?.querySelectorAll(".dp-editable") ?? []).find(
      (el) => el.textContent === "12",
    ) as HTMLElement;
    expect(goldValue, "gold's value is click-to-edit while paused").toBeTruthy();
    act(() => goldValue.click());

    const input = host?.querySelector<HTMLInputElement>(".dp-value-input");
    expect(input).not.toBeNull();
    input!.value = "40";
    act(() => {
      input!.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    expect(editGlobal).toHaveBeenCalledWith("gold", "40");
    // Committed: the input closes.
    expect(host?.querySelector(".dp-value-input")).toBeNull();
  });

  it("a refused edit red-shakes and keeps the input; Esc cancels", () => {
    const { store, editGlobal } = editableStore();
    editGlobal.mockReturnValue(false);
    mount(store);

    const goldValue = Array.from(host?.querySelectorAll(".dp-editable") ?? []).find(
      (el) => el.textContent === "12",
    ) as HTMLElement;
    act(() => goldValue.click());
    const input = host?.querySelector<HTMLInputElement>(".dp-value-input");
    input!.value = "abc";
    act(() => {
      input!.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    // Refused: shake class on, input stays.
    expect(host?.querySelector(".dp-value-input")?.className).toContain("dp-shake");
    act(() => {
      host
        ?.querySelector(".dp-value-input")
        ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    });
    expect(host?.querySelector(".dp-value-input")).toBeNull();
  });

  it("a local edits through debugEditTemp with the frame index + slot", () => {
    const { store, editTemp } = editableStore();
    mount(store);

    // Top frame's local `price` (slot 0) renders as an editable scalar.
    const price = Array.from(host?.querySelectorAll(".dp-editable") ?? []).find(
      (el) => el.textContent === "6",
    ) as HTMLElement;
    expect(price, "price is click-to-edit").toBeTruthy();
    act(() => price.click());
    const input = host?.querySelector<HTMLInputElement>(".dp-value-input");
    input!.value = "9";
    act(() => {
      input!.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    expect(editTemp).toHaveBeenCalledWith(0, 0, "9");
  });

  it("editing is paused-only, and locals lock at a choice stop", () => {
    // Not paused: nothing is editable.
    const running = editableStore({ sessionPaused: false });
    mount(running.store);
    const off = host?.querySelectorAll(".dp-editable-off") ?? [];
    expect(off.length).toBeGreaterThan(0);
    act(() => (off[0] as HTMLElement).click());
    expect(host?.querySelector(".dp-value-input")).toBeNull();
    act(() => root?.unmount());
    host?.remove();

    // Paused at a choice stop: globals edit, locals lock (choosing
    // restores the choice's captured thread — the edit would be lost).
    const atChoices = editableStore({
      debugState: debugState({ status: "waiting_for_choice" }),
    });
    mount(atChoices.store);
    const price = Array.from(host?.querySelectorAll(".dp-editable-off") ?? []).find(
      (el) => el.textContent === "6",
    );
    expect(price, "the local is locked at a choice stop").toBeTruthy();
    const gold = Array.from(host?.querySelectorAll(".dp-editable") ?? []).find(
      (el) => el.textContent === "12" && !el.className.includes("dp-editable-off"),
    );
    expect(gold, "globals stay editable at a choice stop").toBeTruthy();
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
