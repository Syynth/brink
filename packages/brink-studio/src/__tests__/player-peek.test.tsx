/**
 * Peek (ruled 2026-09-03): hovering Continue / a choice card forks the
 * live story, runs ONE continue call on the fork, and the editor bands
 * what it would hit — the live session never moves, the fork is freed.
 */
import { describe, expect, it, afterEach, vi } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { CommandRegistry, ShellProvider } from "@brink/studio-shell";
import {
  createStudioStore,
  LocalSessionProvider,
  isPeekSessionProvider,
  type StudioStore,
} from "@brink/studio-store";
import { PlayerPane, StoreProvider, playerRef } from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const NEXT = { file: "main.ink", range_start: 40, range_end: 52 };
const PICKED = { file: "main.ink", range_start: 90, range_end: 99 };
const CARD = { file: "main.ink", range_start: 70, range_end: 80 };

/** A fake fork: one text line (Continue) or, after `choose`, the picked
 *  branch's first line; records whether it was freed. */
function fakeFork(log: string[]) {
  let chosen: number | null = null;
  return {
    choose: vi.fn((i: number) => {
      chosen = i;
    }),
    currentPath: vi.fn(() => (chosen === null ? "hall" : "hall")),
    advance: vi.fn(() =>
      chosen === null
        ? { type: "text", text: "Next.", tags: [], source: NEXT }
        : { type: "text", text: "Picked.", tags: [], source: PICKED },
    ),
    free: vi.fn(() => {
      log.push("freed");
    }),
  };
}

function fakeSession(status: "running" | "awaiting-choice", log: string[]) {
  return {
    continueSingle: vi.fn(() => {
      log.push("LIVE continueSingle");
      return { type: "end", text: "", tags: [] };
    }),
    continueToPause: vi.fn(() => [{ type: "end", text: "", tags: [] }]),
    currentPath: vi.fn(() => "hall"),
    speculate: vi.fn(() => fakeFork(log)),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    _status: status,
  };
}

function bound(status: "running" | "awaiting-choice") {
  const log: string[] = [];
  const session = fakeSession(status, log);
  const store = createStudioStore();
  const provider = new LocalSessionProvider({ session: session as never, status });
  store.getState()._bindProvider(provider);
  if (status === "awaiting-choice") {
    store.setState({
      sessionChoices: [
        { index: 0, text: "Answer", tags: [], sticky: false, source: CARD },
        { index: 1, text: "Leave", tags: [], sticky: true },
      ],
    } as never);
  }
  return { store, provider, session, log };
}

describe("peek — provider", () => {
  it("the local provider advertises peek; peekContinue forks, advances once, frees, never moves the session", () => {
    const { provider, session, log } = bound("running");
    expect(isPeekSessionProvider(provider)).toBe(true);
    const result = provider.peekContinue();
    expect(result).toEqual({ sources: [NEXT], path: "hall" });
    expect(session.speculate).toHaveBeenCalledTimes(1);
    expect(log).toEqual(["freed"]);
    expect(session.continueSingle).not.toHaveBeenCalled();
  });

  it("peekChoice picks in the fork and reports the branch's first line; refused off a choice point", () => {
    const { provider, session } = bound("awaiting-choice");
    expect(provider.peekChoice(1)?.sources).toEqual([PICKED]);
    expect(provider.peekContinue()).toBeNull(); // Continue cannot be pressed here
    expect(session.choose).not.toHaveBeenCalled(); // the LIVE choose was never called
    const running = bound("running");
    expect(running.provider.peekChoice(0)).toBeNull();
  });

  it("a fork that throws yields no forecast — and is still freed", () => {
    const log: string[] = [];
    const session = fakeSession("running", log);
    session.speculate = vi.fn(() => {
      const fork = fakeFork(log);
      fork.advance = vi.fn(() => {
        throw new Error("runtime error");
      });
      return fork;
    });
    const provider = new LocalSessionProvider({ session: session as never, status: "running" });
    expect(provider.peekContinue()).toBeNull();
    expect(log).toEqual(["freed"]);
  });
});

// ── Player wiring ────────────────────────────────────────────────────

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
const pane = () => createElement(PlayerPane, { doc: playerRef(), groupId: "group-1", active: true });
const over = (el: Element) => act(() => el.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })));
const out = (el: Element) => act(() => el.dispatchEvent(new MouseEvent("mouseout", { bubbles: true })));

describe("peek — Player", () => {
  it("hovering Continue sets the forecast; leaving clears it", () => {
    const { store } = bound("running");
    const el = mount(store, pane());
    const cont = el.querySelector<HTMLButtonElement>("button.player-continue")!;
    over(cont);
    expect(store.getState().sessionPeek).toEqual([NEXT]);
    out(cont);
    expect(store.getState().sessionPeek).toBeNull();
  });

  it("hovering a choice card marks its own text as hover AND its result as peek", () => {
    const { store } = bound("awaiting-choice");
    const el = mount(store, pane());
    const cards = el.querySelectorAll<HTMLButtonElement>(".choices button.player-choice");
    expect(cards.length).toBe(2);
    over(cards[0]);
    expect(store.getState().sessionHoverSource).toEqual(CARD);
    expect(store.getState().sessionPeek).toEqual([PICKED]);
    out(cards[0]);
    expect(store.getState().sessionHoverSource).toBeNull();
    expect(store.getState().sessionPeek).toBeNull();
  });

  it("the forecast is for the state it was taken in: the transcript moving drops it", () => {
    const { store, session } = bound("running");
    session.continueSingle.mockImplementationOnce(() => ({ type: "text", text: "Moved.", tags: [] }));
    store.getState().peekContinue();
    expect(store.getState().sessionPeek).toEqual([NEXT]);
    store.getState().revealNext();
    expect(store.getState().sessionLines.map((l) => l.text)).toEqual(["Moved."]);
    expect(store.getState().sessionPeek).toBeNull();
  });
});
