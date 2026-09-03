/**
 * Restart remounts the Player timeline (feedback 2026-09-02): the store's
 * run generation bumps on start/restart, and the Player keys its spine on
 * it, so the first line fades back in exactly as after Stop → Run instead
 * of the old rows being reused in place.
 */
import { describe, expect, it, afterEach, vi } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { CommandRegistry, ShellProvider } from "@brink/studio-shell";
import { createStudioStore, LocalSessionProvider, type StudioStore } from "@brink/studio-store";
import { PlayerPane, StoreProvider, playerRef } from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

type Line = { type: string; text: string; tags: string[] };

function fakeSession() {
  let n = 0;
  return {
    continueSingle: vi.fn((): Line => ({ type: "text", text: `Line ${(++n).toString()}.`, tags: [] })),
    continueToPause: vi.fn((): Line[] => [{ type: "end", text: "", tags: [] }]),
    currentPath: vi.fn(() => null),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
  };
}

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  if (root) act(() => root!.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(store: StudioStore, ui: ReactNode) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        { commands: new CommandRegistry(), children: null } as never,
        createElement(StoreProvider, { store, children: ui } as never),
      ),
    );
  });
  return container;
}

describe("restart remounts the Player timeline", () => {
  it("the store's run generation bumps on restart and on start", () => {
    const store = createStudioStore();
    const session = fakeSession();
    const provider = new LocalSessionProvider({ session: session as never, status: "running" });
    store.getState()._bindProvider(provider);
    const before = store.getState().sessionRun;

    store.getState().revealNext();
    expect(store.getState().sessionLines).toHaveLength(1);
    expect(store.getState().sessionRun).toBe(before); // a reveal is the same run

    store.getState().restartSession();
    expect(session.restart).toHaveBeenCalledTimes(1);
    expect(store.getState().sessionRun).toBe(before + 1);
  });

  it("a new run replaces the row elements, so the entrance animation plays again", () => {
    const store = createStudioStore();
    store.setState({
      sessionStatus: "running",
      sessionText: ["The lights dim."],
      sessionLines: [{ text: "The lights dim.", kind: "line" as const, tags: [] }],
    } as never);
    const el = mount(store, createElement(PlayerPane, { doc: playerRef(), groupId: "group-1", active: true }));
    const first = el.querySelector<HTMLDivElement>(".player-line-row")!;
    const spine = el.querySelector<HTMLDivElement>(".player-spine")!;

    // Same run, more lines: the existing row is kept (no re-entrance).
    act(() => {
      store.setState({
        sessionText: ["The lights dim.", "A door."],
        sessionLines: [
          { text: "The lights dim.", kind: "line" as const, tags: [] },
          { text: "A door.", kind: "line" as const, tags: [] },
        ],
      } as never);
    });
    expect(el.querySelector(".player-line-row")).toBe(first);
    expect(el.querySelector(".player-spine")).toBe(spine);

    // New run: the whole spine — start marker, rows, choices — is fresh DOM.
    act(() => {
      store.setState({
        sessionRun: store.getState().sessionRun + 1,
        sessionText: ["The lights dim."],
        sessionLines: [{ text: "The lights dim.", kind: "line" as const, tags: [] }],
      } as never);
    });
    const again = el.querySelector<HTMLDivElement>(".player-line-row")!;
    expect(again).not.toBe(first);
    expect(first.isConnected).toBe(false);
    expect(el.querySelector(".player-spine")).not.toBe(spine);
    expect(again.textContent).toContain("The lights dim.");
  });
});
