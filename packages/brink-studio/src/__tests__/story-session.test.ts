/**
 * Story session model + lifecycle commands (spec §7.6, shell issue 2.1).
 *
 * Covers: session status transitions, the SessionProvider seam (#179), `when`
 * gating of story.start/restart/stop/choose/continue per status, stop →
 * placeholder state, and failed-compile-keeps-session.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  createStudioStore,
  LocalSessionProvider,
  REPLAY_DIVERGED_MESSAGE,
  type StudioStore,
} from "@brink/studio-store";
import { CommandRegistry } from "@brink/studio-shell";
import { registerStoryCommands } from "../story-commands.js";

type Line = {
  type: string;
  text: string;
  tags: string[];
  choices?: { index: number; text: string; tags: string[] }[];
};

/** A scripted runner: each continueSingle() call consumes the next line. */
function scriptedRunner(lines: Line[]) {
  let i = 0;
  return {
    continueSingle: vi.fn((): Line => {
      const line = lines[i];
      if (!line) throw new Error("script exhausted");
      i += 1;
      return line;
    }),
    choose: vi.fn(),
    reset: vi.fn(),
    free: vi.fn(),
  };
}

/**
 * Bind a LocalSessionProvider wrapping `runner` to the store, mirroring its
 * snapshot into the reactive fields. Models "the studio already has a live
 * runner" — the runner is a provider implementation detail (#179).
 */
function bindRunner(
  store: StudioStore,
  runner: Record<string, unknown>,
  opts: { status?: Line["type"] | "running" | "awaiting-choice" | "ended"; transcript?: string[]; choices?: Line["choices"] } = {},
): LocalSessionProvider {
  const provider = new LocalSessionProvider({
    runner: runner as never,
    status: (opts.status ?? "running") as never,
    transcript: opts.transcript,
    choices: (opts.choices ?? []) as never,
  });
  store.getState()._bindProvider(provider);
  return provider;
}

beforeEach(() => {
  localStorage.clear();
});

describe("session slice split", () => {
  it("starts with no session and keeps only UI state in the player slice", () => {
    const store = createStudioStore();
    const s = store.getState();

    expect(s.sessionStatus).toBe("none");
    expect(s.sessionText).toEqual([]);
    expect(s.sessionChoices).toEqual([]);
    expect(s._provider).toBeNull();
    // playerFullscreen is gone too (#86): maximize is a shell feature now.
    expect(s).not.toHaveProperty("playerFullscreen");

    // The legacy PlayerSlice session fields are gone — the session is its
    // own object now.
    expect(s).not.toHaveProperty("playerText");
    expect(s).not.toHaveProperty("playerChoices");
    expect(s).not.toHaveProperty("playerEnded");
    expect(s).not.toHaveProperty("playerCanContinue");
    expect(s).not.toHaveProperty("loadStory");
    expect(s).not.toHaveProperty("resetStory");
    expect(s).not.toHaveProperty("disposePlayer");
  });
});

describe("session status transitions", () => {
  it("walks running → awaiting-choice → running → ended", () => {
    const store = createStudioStore();
    const runner = scriptedRunner([
      { type: "text", text: "intro\n", tags: [] },
      { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Go", tags: [] }] },
      { type: "text", text: "after\n", tags: [] },
      { type: "end", text: "fin\n", tags: [] },
    ]);
    const provider = bindRunner(store, runner, { status: "running" });

    store.getState().revealNext();
    expect(store.getState().sessionStatus).toBe("running");

    store.getState().revealNext();
    expect(store.getState().sessionStatus).toBe("awaiting-choice");
    expect(store.getState().sessionChoices).toHaveLength(1);

    // chooseOption applies the choice, records it, and reveals the next line.
    store.getState().chooseOption(0);
    expect(runner.choose).toHaveBeenCalledWith(0);
    expect(store.getState().sessionStatus).toBe("running");
    expect(provider.recordedChoices).toEqual([0]);
    expect(store.getState().sessionText).toContain("> Go");

    store.getState().revealNext();
    expect(store.getState().sessionStatus).toBe("ended");
  });

  it("transitions to error when choose throws", () => {
    const store = createStudioStore();
    const runner = scriptedRunner([]);
    runner.choose.mockImplementation(() => {
      throw new Error("bad choice");
    });
    bindRunner(store, runner, {
      status: "awaiting-choice",
      choices: [{ index: 0, text: "Go", tags: [] }],
    });

    store.getState().chooseOption(0);
    expect(store.getState().sessionStatus).toBe("error");
  });

  it("stopSession clears the run and returns to none (placeholder state)", () => {
    const store = createStudioStore();
    const runner = scriptedRunner([]);
    localStorage.setItem("brink-player-save", JSON.stringify({ choiceLog: [1] }));
    const bytes = new Uint8Array([1, 2, 3]);
    const provider = bindRunner(store, runner, {
      status: "awaiting-choice",
      transcript: ["intro"],
      choices: [{ index: 0, text: "Go", tags: [] }],
    });
    store.setState({ _sessionBytes: bytes });

    store.getState().stopSession();

    const s = store.getState();
    expect(runner.free).toHaveBeenCalled();
    expect(s.sessionStatus).toBe("none");
    expect(s.sessionText).toEqual([]);
    expect(s.sessionChoices).toEqual([]);
    expect(provider.recordedChoices).toEqual([]);
    // The provider is kept (program identity outlives the run) but its runner
    // is freed — a later story.start re-creates one.
    expect(s._provider).toBe(provider);
    expect(provider.hasLiveRunner()).toBe(false);
    expect(localStorage.getItem("brink-player-save")).toBeNull();
    // Program identity survives the stop so story.start can run it again.
    expect(s._sessionBytes).toBe(bytes);
  });

  it("restartSession resets the runner, clears the log, and reveals fresh", () => {
    const store = createStudioStore();
    const runner = scriptedRunner([{ type: "text", text: "from the top\n", tags: [] }]);
    const provider = bindRunner(store, runner, { status: "ended", transcript: ["old"] });

    store.getState().restartSession();

    const s = store.getState();
    expect(runner.reset).toHaveBeenCalled();
    expect(s.sessionStatus).toBe("running");
    expect(s.sessionText).toEqual(["from the top"]);
    expect(provider.recordedChoices).toEqual([]);
  });
});

describe("story lifecycle command gating (spec §7.6)", () => {
  function setup() {
    const store = createStudioStore();
    const commands = new CommandRegistry();
    registerStoryCommands(commands, store);
    return { store, commands };
  }

  it("disables everything with no session and no program", () => {
    const { commands } = setup();
    for (const id of ["story.start", "story.restart", "story.stop", "story.choose", "story.continue"]) {
      expect(commands.isEnabled(id), id).toBe(false);
    }
  });

  it("enables start/restart once compiled bytes exist (status none)", () => {
    const { store, commands } = setup();
    store.setState({ storyBytes: new Uint8Array([0]) });

    expect(commands.isEnabled("story.start")).toBe(true);
    expect(commands.isEnabled("story.restart")).toBe(true);
    expect(commands.isEnabled("story.stop")).toBe(false);
    expect(commands.isEnabled("story.choose")).toBe(false);
    expect(commands.isEnabled("story.continue")).toBe(false);
  });

  it("gates per status while a session runs", () => {
    const { store, commands } = setup();
    // Gating reads only sessionStatus (+ available program bytes), so the
    // status alone drives the predicates — no runner needed.
    store.setState({ sessionStatus: "running" });

    expect(commands.isEnabled("story.start")).toBe(false); // session exists
    expect(commands.isEnabled("story.restart")).toBe(true);
    expect(commands.isEnabled("story.stop")).toBe(true);
    expect(commands.isEnabled("story.choose")).toBe(false);
    expect(commands.isEnabled("story.continue")).toBe(true);

    store.setState({ sessionStatus: "awaiting-choice" });
    expect(commands.isEnabled("story.choose")).toBe(true);
    expect(commands.isEnabled("story.continue")).toBe(false);

    store.setState({ sessionStatus: "done" });
    expect(commands.isEnabled("story.choose")).toBe(false);
    expect(commands.isEnabled("story.continue")).toBe(true); // DONE is a turn boundary

    store.setState({ sessionStatus: "ended" });
    expect(commands.isEnabled("story.continue")).toBe(false);
    expect(commands.isEnabled("story.restart")).toBe(true);
    expect(commands.isEnabled("story.stop")).toBe(true);
  });

  it("story.choose dispatches the chosen index into the session", () => {
    const { store, commands } = setup();
    const runner = scriptedRunner([{ type: "text", text: "picked\n", tags: [] }]);
    bindRunner(store, runner, {
      status: "awaiting-choice",
      choices: [
        { index: 0, text: "A", tags: [] },
        { index: 1, text: "B", tags: [] },
      ],
    });

    expect(commands.dispatch("story.choose", 1)).toBe(true);
    expect(runner.choose).toHaveBeenCalledWith(1);
    expect(store.getState().sessionText).toContain("> B");
  });

  it("story.choose is refused outside awaiting-choice", () => {
    const { store, commands } = setup();
    const runner = scriptedRunner([]);
    bindRunner(store, runner, { status: "running" });

    expect(commands.dispatch("story.choose", 0)).toBe(false);
    expect(runner.choose).not.toHaveBeenCalled();
  });

  it("story.stop then story.start runs the kept program again", () => {
    const { store, commands } = setup();
    const runner = scriptedRunner([]);
    const bytes = new Uint8Array([9]);
    bindRunner(store, runner, { status: "running" });
    store.setState({
      _sessionBytes: bytes,
      // No compiled bytes — e.g. the latest compile failed. The session's own
      // program identity keeps start workable.
      storyBytes: null,
    });

    expect(commands.dispatch("story.stop")).toBe(true);
    expect(store.getState().sessionStatus).toBe("none");
    expect(commands.isEnabled("story.start")).toBe(true);

    // start re-creates a real (mock-wasm) runner on the kept bytes; the mock
    // story ends immediately, but a session now exists again.
    expect(commands.dispatch("story.start")).toBe(true);
    expect(store.getState().sessionStatus).not.toBe("none");
  });
});

describe("recompile-while-running", () => {
  it("a failed compile leaves the existing session untouched", () => {
    const store = createStudioStore();
    const runner = scriptedRunner([]);
    const provider = bindRunner(store, runner, {
      status: "awaiting-choice",
      transcript: ["intro"],
      choices: [{ index: 0, text: "Go", tags: [] }],
    });
    store.setState({ storyBytes: new Uint8Array([1]) });

    // What main.tsx does on a failed compile: record diagnostics with null
    // bytes and do NOT touch the session.
    store.getState().setCompileResult([], { errors: 1, warnings: 0 }, [], null);

    const s = store.getState();
    expect(s.storyBytes).toBeNull();
    expect(s._provider).toBe(provider);
    expect(s.sessionStatus).toBe("awaiting-choice");
    expect(s.sessionText).toEqual(["intro"]);
    expect(s.sessionChoices).toHaveLength(1);
    expect(runner.free).not.toHaveBeenCalled();
  });

  it("startSession replays the persisted choice log on the new program", () => {
    // Persist a one-choice log, then start. The mock wasm StoryRunner ends
    // immediately, so the replay diverges (the recorded choice is
    // unreachable) — it must truncate and notify rather than loop or crash.
    localStorage.setItem("brink-player-save", JSON.stringify({ choiceLog: [0] }));
    const store = createStudioStore();
    const notify = vi.fn();
    store.getState().setNotifier(notify);

    store.getState().startSession(new Uint8Array([1]));

    const s = store.getState();
    expect(s.sessionStatus).toBe("ended"); // mock story ends at once
    expect((s._provider as LocalSessionProvider).recordedChoices).toEqual([]);
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({
        severity: "warning",
        source: "story",
        message: REPLAY_DIVERGED_MESSAGE,
      }),
    );
  });
});
