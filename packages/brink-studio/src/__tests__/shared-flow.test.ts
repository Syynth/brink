/**
 * Shared-context flow sessions (#200) — "+ New flow".
 *
 * A flow session drives a concurrent flow of the *primary* session's story,
 * sharing its globals. In the studio it's a {@link FlowSessionProvider} in the
 * same registry as the independent-runner sessions (#182); the runtime/wasm
 * sharing is proven in brink-runtime + brink-web tests.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  createStudioStore,
  FlowSessionProvider,
  DEFAULT_SESSION_ID,
} from "@brink/studio-store";
import { CommandRegistry } from "@brink/studio-shell";
import { registerStoryCommands } from "../story-commands.js";

type Line = { type: string; text: string; tags: string[]; choices?: { index: number; text: string; tags: string[] }[] };

beforeEach(() => localStorage.clear());

describe("openFlow (store)", () => {
  function withPrimary() {
    const store = createStudioStore();
    store.setState({ storyBytes: new Uint8Array([1]) });
    store.getState().startSession(new Uint8Array([1])); // primary (mock ends)
    return store;
  }

  it("spawns a flow session sharing the primary's story, made active", () => {
    const store = withPrimary();
    expect(store.getState().sessions).toHaveLength(1);

    store.getState().openFlow();

    const s = store.getState();
    expect(s.sessions).toHaveLength(2);
    expect(s.sessions[1]!.id).toBe("flow:1");
    expect(s.activeSessionId).toBe("flow:1");
    // A flow drives its own choices/continue; the shared story owns start/stop.
    // `auto` joined the set in #3011: a flow can run to the next pause via
    // `continueFlowMaximally`, so it advertises the reveal-mode toggle too.
    expect([...s.capabilities].sort()).toEqual(["auto", "choose", "continue"]);
  });

  it("does nothing without a live primary session", () => {
    const store = createStudioStore();
    store.setState({ storyBytes: new Uint8Array([1]) });
    store.getState().openFlow(); // no primary started
    expect(store.getState().sessions).toHaveLength(0);
  });

  it("closing a flow falls back to the primary", () => {
    const store = withPrimary();
    store.getState().openFlow();
    store.getState().closeSession("flow:1");
    expect(store.getState().sessions).toHaveLength(1);
    expect(store.getState().activeSessionId).toBe(DEFAULT_SESSION_ID);
  });

  it("drops stale flow sessions on recompile (the primary's story is replaced)", () => {
    const store = withPrimary();
    store.getState().openFlow();
    expect(store.getState().sessions).toHaveLength(2);

    // A recompile reloads the primary → its shared flows are gone.
    store.getState().startSession(new Uint8Array([2]));

    const s = store.getState();
    expect(s.sessions.map((e) => e.id)).toEqual([DEFAULT_SESSION_ID]);
    expect(s.activeSessionId).toBe(DEFAULT_SESSION_ID);
  });
});

describe("FlowSessionProvider", () => {
  function fakeRunner(line: Line) {
    return {
      programModel: () => ({ checksum: "0xshared" }),
      programInkt: () => "inkt",
      continueFlow: vi.fn(() => line),
      chooseFlow: vi.fn(),
      destroyFlow: vi.fn(),
      flowDebugSnapshot: vi.fn(() => null),
    };
  }

  it("drives the named flow and reports the shared program identity", () => {
    const runner = fakeRunner({ type: "end", text: "hi\n", tags: [] });
    const provider = new FlowSessionProvider(runner as never, "f");
    let snap = provider.getSnapshot();
    provider.subscribe((s) => (snap = s));

    provider.start();
    expect(runner.continueFlow).toHaveBeenCalledWith("f");
    expect(snap.transcript.map((l) => l.text)).toEqual(["hi"]);
    expect(snap.status).toBe("ended");
    expect(snap.programChecksum).toBe("0xshared");
  });

  it("applies a choice then reveals the next line", () => {
    const runner = fakeRunner({ type: "end", text: "after\n", tags: [] });
    const provider = new FlowSessionProvider(runner as never, "f");
    // Seed a pending choice by revealing a choices line first.
    runner.continueFlow.mockReturnValueOnce({
      type: "choices",
      text: "",
      tags: [],
      choices: [{ index: 0, text: "Go", tags: [] }],
    });
    provider.start();
    provider.choose(0);

    expect(runner.chooseFlow).toHaveBeenCalledWith("f", 0);
    expect(provider.getSnapshot().transcript.map((l) => l.text)).toContain("> Go");
  });

  it("destroys the flow on dispose", () => {
    const runner = fakeRunner({ type: "end", text: "", tags: [] });
    const provider = new FlowSessionProvider(runner as never, "f");
    provider.dispose();
    expect(runner.destroyFlow).toHaveBeenCalledWith("f");
  });
});

describe("story.openFlow command", () => {
  it("is gated on a live session and dispatches openFlow", () => {
    const store = createStudioStore();
    const commands = new CommandRegistry();
    registerStoryCommands(commands, store);

    expect(commands.isEnabled("story.openFlow")).toBe(false); // no session
    store.setState({ storyBytes: new Uint8Array([1]) });
    store.getState().startSession(new Uint8Array([1]));
    expect(commands.isEnabled("story.openFlow")).toBe(true);

    expect(commands.dispatch("story.openFlow")).toBe(true);
    expect(store.getState().sessions.some((e) => e.id.startsWith("flow:"))).toBe(true);
  });
});
