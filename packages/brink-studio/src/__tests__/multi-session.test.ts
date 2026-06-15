/**
 * Multi-session registry + picker (docs/multi-session-spec.md, #182).
 *
 * The store holds a registry of `SessionProvider`s and an active id; the
 * session-bound views mirror the *active* session (so they never change).
 * Local multi-session uses independent runners with isolated globals — opening
 * a secondary session must not persist over the primary's saved choice log.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  createStudioStore,
  LocalSessionProvider,
  DEFAULT_SESSION_ID,
} from "@brink/studio-store";
import { CommandRegistry } from "@brink/studio-shell";
import { registerStoryCommands } from "../story-commands.js";

type Line = {
  type: string;
  text: string;
  tags: string[];
  choices?: { index: number; text: string; tags: string[] }[];
};

const SAVE_KEY = "brink-player-save";

/** A provider wrapping an adopted runner already at a known snapshot. */
function provider(
  status: Line["type"] | "running" | "awaiting-choice" | "ended",
  transcript: string[],
  choices: Line["choices"] = [],
): LocalSessionProvider {
  return new LocalSessionProvider({
    runner: { continueSingle: () => ({ type: "end", text: "", tags: [] }) } as never,
    status: status as never,
    transcript,
    choices: choices as never,
  });
}

beforeEach(() => {
  localStorage.clear();
});

describe("session registry — switching repoints the views", () => {
  it("mirrors the active session's snapshot; switching repoints", () => {
    const store = createStudioStore();
    const main = provider("running", ["main line"]);
    const other = provider("awaiting-choice", ["other line"], [
      { index: 0, text: "pick", tags: [] },
    ]);
    store.setState({
      sessions: [
        { id: DEFAULT_SESSION_ID, label: "Main", provider: main },
        { id: "local:1", label: "Other", provider: other },
      ],
    });

    store.getState().setActiveSession(DEFAULT_SESSION_ID);
    expect(store.getState().activeSessionId).toBe(DEFAULT_SESSION_ID);
    expect(store.getState().sessionText).toEqual(["main line"]);
    expect(store.getState().sessionStatus).toBe("running");
    expect(store.getState().sessionChoices).toEqual([]);

    store.getState().setActiveSession("local:1");
    expect(store.getState().activeSessionId).toBe("local:1");
    expect(store.getState().sessionText).toEqual(["other line"]);
    expect(store.getState().sessionStatus).toBe("awaiting-choice");
    expect(store.getState().sessionChoices).toHaveLength(1);
    // A switch is a different timeline — the step diff resets.
    expect(store.getState().prevDebugState).toBeNull();
  });
});

describe("opening + closing local sessions", () => {
  it("opens a secondary session, makes it active, and falls back on close", () => {
    const store = createStudioStore();
    store.setState({ storyBytes: new Uint8Array([1]) });

    // Primary session (the mock wasm story ends immediately).
    store.getState().startSession(new Uint8Array([1]));
    expect(store.getState().sessions).toHaveLength(1);
    expect(store.getState().activeSessionId).toBe(DEFAULT_SESSION_ID);

    // Open a second, independent session — registered + active.
    store.getState().openSession();
    expect(store.getState().sessions).toHaveLength(2);
    expect(store.getState().activeSessionId).toBe("local:1");

    // Switch back, then close the secondary → fall back to what remains.
    store.getState().setActiveSession(DEFAULT_SESSION_ID);
    store.getState().closeSession("local:1");
    expect(store.getState().sessions).toHaveLength(1);
    expect(store.getState().sessions.map((s) => s.id)).toEqual([DEFAULT_SESSION_ID]);
  });

  it("closing the active secondary falls back to a remaining session", () => {
    const store = createStudioStore();
    store.setState({ storyBytes: new Uint8Array([1]) });
    store.getState().startSession(new Uint8Array([1]));
    store.getState().openSession(); // local:1, active

    store.getState().closeSession("local:1");
    expect(store.getState().sessions).toHaveLength(1);
    expect(store.getState().activeSessionId).toBe(DEFAULT_SESSION_ID);
  });

  it("refuses to close the primary session", () => {
    const store = createStudioStore();
    store.setState({ storyBytes: new Uint8Array([1]) });
    store.getState().startSession(new Uint8Array([1]));

    store.getState().closeSession(DEFAULT_SESSION_ID);
    expect(store.getState().sessions).toHaveLength(1);
    expect(store.getState().activeSessionId).toBe(DEFAULT_SESSION_ID);
  });

  it("does nothing without a program to play", () => {
    const store = createStudioStore();
    store.getState().openSession();
    expect(store.getState().sessions).toHaveLength(0);
  });
});

describe("secondary sessions are isolated (no persistence)", () => {
  it("a non-persistent session does not write the saved choice log", () => {
    const runner = {
      continueSingle: (): Line => ({ type: "end", text: "", tags: [] }),
      choose: vi.fn(),
    };
    const secondary = new LocalSessionProvider({
      runner: runner as never,
      status: "awaiting-choice",
      choices: [{ index: 0, text: "go", tags: [] }] as never,
      persist: false,
    });

    secondary.choose(0);

    expect(runner.choose).toHaveBeenCalledWith(0);
    expect(secondary.recordedChoices).toEqual([0]); // tracked in-memory…
    expect(localStorage.getItem(SAVE_KEY)).toBeNull(); // …but never persisted
  });

  it("the primary (persistent) session does write the saved log", () => {
    const runner = {
      continueSingle: (): Line => ({ type: "end", text: "", tags: [] }),
      choose: vi.fn(),
    };
    const primary = new LocalSessionProvider({
      runner: runner as never,
      status: "awaiting-choice",
      choices: [{ index: 0, text: "go", tags: [] }] as never,
    });

    primary.choose(0);

    expect(JSON.parse(localStorage.getItem(SAVE_KEY)!)).toEqual({ choiceLog: [0] });
  });
});

describe("story.openSession command", () => {
  it("is enabled when a program exists and dispatches a new session", () => {
    const store = createStudioStore();
    const commands = new CommandRegistry();
    registerStoryCommands(commands, store);

    expect(commands.isEnabled("story.openSession")).toBe(false); // no program yet
    store.setState({ storyBytes: new Uint8Array([1]) });
    expect(commands.isEnabled("story.openSession")).toBe(true);

    expect(commands.dispatch("story.openSession")).toBe(true);
    expect(store.getState().sessions.length).toBeGreaterThanOrEqual(1);
  });
});
