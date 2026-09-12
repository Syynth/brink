/**
 * Tabbing out of the Player and back.
 *
 * The shell renders only a group's active tab (`editor-area.tsx` mounts
 * `activeTab(group)` alone), so switching to another tab UNMOUNTS the Player
 * and switching back mounts a fresh one. The session lives in the store, not
 * in the pane, so the transcript itself survives that untouched — the first
 * test pins exactly that, because it is the load-bearing assumption behind
 * the rest.
 *
 * What did NOT survive is presentation. Every row is a new DOM node after a
 * remount, so the `player-row-in` arrival animation replayed for the whole
 * transcript (`both` fill mode blanks each row for the 240ms delay first),
 * and the scroll position was gone — an author who had scrolled up to read
 * back got dragged to the bottom on every tab switch. Both are fixed by
 * marking already-delivered rows settled and remembering the scroll offset
 * per view; a restart still plays the entrance from the first line.
 */

import { describe, expect, it, afterEach, vi } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { CommandRegistry, ShellProvider } from "@brink/studio-shell";
import {
  createStudioStore,
  LocalSessionProvider,
  type StudioStore,
} from "@brink/studio-store";
import { PlayerPane, StoreProvider, playerRef } from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

/** A session that hands out "line 1", "line 2", … and logs every call, so a
 *  remount touching the story at all is visible rather than assumed. */
function scriptedSession(log: string[]) {
  let n = 0;
  const note =
    (name: string) =>
    <T,>(value: T): T => {
      log.push(name);
      return value;
    };
  return {
    continueSingle: vi.fn(() => {
      log.push("continueSingle");
      n += 1;
      return { type: "text", text: `line ${n.toString()}\n`, tags: [] };
    }),
    continueToPause: vi.fn(() => [{ type: "done", text: "", tags: [] }]),
    currentPath: vi.fn(() => "hall"),
    choose: vi.fn(() => note("choose")(undefined)),
    restart: vi.fn(() => note("restart")(undefined)),
    free: vi.fn(() => note("free")(undefined)),
    reload: vi.fn(() => note("reload")({ type: "replayed" })),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    programModel: vi.fn(() => null),
    programInkt: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    saveState: vi.fn(() => null),
    exportTranscript: vi.fn(() => null),
  };
}

function running(lines: number) {
  const log: string[] = [];
  const session = scriptedSession(log);
  const store = createStudioStore();
  const provider = new LocalSessionProvider({
    session: session as never,
    status: "running",
  });
  store.getState()._bindProvider(provider);
  act(() => {
    for (let i = 0; i < lines; i++) store.getState().revealNext();
  });
  return { store, provider, session, log };
}

let root: Root | null = null;
let container: HTMLDivElement | null = null;
afterEach(() => {
  if (root) act(() => root!.unmount());
  container?.remove();
  root = null;
  container = null;
});

function tree(store: StudioStore, ui: ReactNode) {
  return createElement(
    ShellProvider,
    { commands: new CommandRegistry(), children: null } as never,
    createElement(StoreProvider, { store, children: ui } as never),
  );
}

const pane = (groupId = "group-1") =>
  createElement(PlayerPane, { doc: playerRef(), groupId, active: true });

function mount(store: StudioStore, ui: ReactNode) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => root!.render(tree(store, ui)));
  return container;
}

/** Switch to another tab (the Player unmounts), then back to it. */
function tabAwayAndBack(store: StudioStore, ui: ReactNode): void {
  act(() => root!.render(createElement("div", null, "another tab")));
  act(() => root!.render(tree(store, ui)));
}

const rows = (el: Element): Element[] => [...el.querySelectorAll(".player-line-row")];
const texts = (el: Element): string[] =>
  rows(el).map((n) => n.querySelector("p")?.textContent ?? "");
const settled = (el: Element): boolean[] =>
  rows(el).map((n) => n.classList.contains("is-settled"));

describe("tabbing out of the Player and back", () => {
  it("leaves the transcript intact and never touches the session", () => {
    const { store, log } = running(3);
    const el = mount(store, pane());
    const before = texts(el);
    expect(before).toEqual(["line 1", "line 2", "line 3"]); // non-vacuity
    const callsBefore = log.length;

    tabAwayAndBack(store, pane());

    expect(texts(container!)).toEqual(before);
    // Nothing about tabbing may reach the story: no reload, no restart, no
    // stray continue. This is what rules the session layer out of the bug.
    expect(log.slice(callsBefore)).toEqual([]);
  });

  it("does not replay the arrival animation for lines already read", () => {
    // The real path: the pane is open and the lines arrive into it.
    const { store } = running(0);
    const el = mount(store, pane());
    act(() => {
      for (let i = 0; i < 3; i++) store.getState().revealNext();
    });
    // Each one arrived while mounted, so each animated in — unchanged.
    expect(texts(el)).toEqual(["line 1", "line 2", "line 3"]); // non-vacuity
    expect(settled(el)).toEqual([false, false, false]);

    tabAwayAndBack(store, pane());

    // After the remount all three are settled: the author re-reads them where
    // they were, with no blank-and-fade.
    expect(settled(container!)).toEqual([true, true, true]);
  });

  it("opening the Player onto a story already in progress does not fade it in", () => {
    // Reopening a closed Player tab, or the studio restoring its persisted
    // journal at startup, lands a whole transcript at once. None of it is
    // arriving — it already happened — so it renders settled rather than
    // playing the entrance for every line.
    const { store } = running(3);
    const el = mount(store, pane());
    expect(texts(el)).toEqual(["line 1", "line 2", "line 3"]); // non-vacuity
    expect(settled(el)).toEqual([true, true, true]);
  });

  it("still animates lines that arrive after the remount", () => {
    const { store } = running(2);
    mount(store, pane());
    tabAwayAndBack(store, pane());
    expect(settled(container!)).toEqual([true, true]);

    act(() => store.getState().revealNext());

    // The two restored lines stay settled; the new one arrives and animates.
    expect(texts(container!)).toEqual(["line 1", "line 2", "line 3"]);
    expect(settled(container!)).toEqual([true, true, false]);
  });

  it("a restart replays the entrance from the first line", () => {
    const { store } = running(2);
    mount(store, pane());
    tabAwayAndBack(store, pane());
    expect(settled(container!)).toEqual([true, true]);

    // Stop → Run is a fresh timeline (`sessionRun` bumps): the settled count
    // resets, so the story fades in from its first line exactly as before.
    act(() => {
      store.setState({ sessionRun: store.getState().sessionRun + 1 } as never);
      store.getState().revealNext();
    });

    expect(settled(container!).slice(-1)).toEqual([false]);
  });

  it("keeps the scroll position of an author who scrolled up to read back", () => {
    const { store } = running(5);
    const el = mount(store, pane());
    const scroller = el.querySelector<HTMLDivElement>(".player");
    expect(scroller).not.toBeNull();

    // jsdom reports zero heights, so drive the two inputs the pane actually
    // reads: a scroll event with the geometry of "scrolled well up".
    Object.defineProperty(scroller, "scrollHeight", { value: 1000, configurable: true });
    Object.defineProperty(scroller, "clientHeight", { value: 200, configurable: true });
    scroller!.scrollTop = 120;
    act(() => scroller!.dispatchEvent(new Event("scroll", { bubbles: true })));

    tabAwayAndBack(store, pane());

    const after = container!.querySelector<HTMLDivElement>(".player");
    expect(after).not.toBeNull();
    expect(after!.scrollTop).toBe(120);
  });

  it("a split-duplicated Player keeps a scroll position per view", () => {
    const { store } = running(5);
    // Two views over the one session document, as an explicit split gives.
    const both = (): ReactNode =>
      createElement("div", null, pane("group-1"), pane("group-2"));
    const el = mount(store, both());
    const [left, right] = [...el.querySelectorAll<HTMLDivElement>(".player")];
    for (const s of [left, right]) {
      Object.defineProperty(s, "scrollHeight", { value: 1000, configurable: true });
      Object.defineProperty(s, "clientHeight", { value: 200, configurable: true });
    }
    left.scrollTop = 90;
    right.scrollTop = 310;
    act(() => {
      left.dispatchEvent(new Event("scroll", { bubbles: true }));
      right.dispatchEvent(new Event("scroll", { bubbles: true }));
    });

    tabAwayAndBack(store, both());

    const after = [...container!.querySelectorAll<HTMLDivElement>(".player")];
    expect(after.map((s) => s.scrollTop)).toEqual([90, 310]);
  });
});

describe("the settled count tracks the transcript it indexes", () => {
  it("a truncated transcript still animates what arrives next", () => {
    const { store } = running(0);
    const el = mount(store, pane());
    act(() => {
      for (let i = 0; i < 4; i++) store.getState().revealNext();
    });
    tabAwayAndBack(store, pane());
    expect(settled(container!)).toEqual([true, true, true, true]);

    // A divergent replay truncates in place, without bumping the run — the
    // store mirrors a SHORTER transcript than the one already on screen.
    const kept = store.getState().sessionLines.slice(0, 2);
    act(() => {
      store.setState({ sessionLines: kept } as never);
    });
    expect(texts(container!)).toEqual(["line 1", "line 2"]); // non-vacuity

    // Play on from there: the next line the mirror delivers.
    act(() => {
      store.setState({
        sessionLines: [...kept, { text: "line 5", kind: "text", tags: [] }],
      } as never);
    });

    // The replacement line is genuinely new: it must animate, not inherit a
    // settled slot from the transcript that was thrown away.
    expect(texts(container!)).toEqual(["line 1", "line 2", "line 5"]);
    expect(settled(container!)).toEqual([true, true, false]);
  });
});
