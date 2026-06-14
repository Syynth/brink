import { describe, it, expect, vi, beforeEach } from "vitest";
import { LocalSessionProvider, REPLAY_DIVERGED_MESSAGE } from "@brink/studio-store";

// The LocalSessionProvider fast-forwards a persisted choice log when a session
// starts (#179: the replay-on-recompile path, spec §6.1/§7.6). Driving it
// through `start()` with a seeded save exercises the real entry point. Two
// behaviors are pinned:
//
// 1. Termination: a story that reaches a `-> DONE` dead-end before the next
//    saved choice used to spin continueStory() forever and lock the UI thread
//    (#7). The iteration cap + consumedChoice bail guarantee it returns.
// 2. Divergence (spec §7.6): when a recorded choice no longer applies, the
//    history is *truncated at the divergence point* (not discarded wholesale),
//    the session stays at the position it reached, and a "warning" from source
//    "story" goes through the injected notifier (the store→shell bridge, §7.5).

type Line = {
  type: string;
  text: string;
  tags: string[];
  choices?: { index: number; text: string; tags: string[] }[];
};

const SAVE_KEY = "brink-player-save";

/** A scripted runner with the full surface the provider drives during replay. */
function fullRunner(overrides: Record<string, unknown>): Record<string, unknown> {
  return {
    continueSingle: (): Line => ({ type: "end", text: "", tags: [] }),
    continueStory: (): Line[] => [{ type: "end", text: "", tags: [] }],
    choose: vi.fn(),
    reset: vi.fn(),
    // start() reloads the adopted runner in place; has_recording=false means
    // the re-walk runs live (the mock records nothing) — today's behavior.
    reload: vi.fn(),
    hasRecording: () => false,
    free: vi.fn(),
    ...overrides,
  };
}

/** Bind a runner to a provider, seed the saved log, and start (triggers replay). */
function startWithSavedLog(runner: Record<string, unknown>, choiceLog: number[]) {
  const notify = vi.fn();
  localStorage.setItem(SAVE_KEY, JSON.stringify({ choiceLog }));
  const provider = new LocalSessionProvider({ runner: runner as never, status: "running" });
  provider.setCallbacks({ notify, appendOutput: vi.fn() });
  provider.start(new Uint8Array([1]));
  return { provider, notify, snap: () => provider.getSnapshot() };
}

/** The divergence warning the bridge must receive (severity + source pinned). */
const DIVERGED_NOTIFICATION = {
  severity: "warning",
  source: "story",
  message: REPLAY_DIVERGED_MESSAGE,
};

describe("LocalSessionProvider replay", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("terminates and truncates when the story dead-ends on DONE before a saved choice", () => {
    // Always returns text + a DONE terminal, never a choice or end.
    const runner = fullRunner({
      continueStory: (): Line[] => [
        { type: "text", text: "stuck\n", tags: [] },
        { type: "done", text: "", tags: [] },
      ],
    });
    const { provider, notify, snap } = startWithSavedLog(runner, [0, 1]);

    // Without the guard this never returns. The iteration cap + consumedChoice
    // bail guarantee it does.
    expect(runner.choose).not.toHaveBeenCalled();
    // Divergence at choice 0: nothing kept, session stays at the DONE boundary
    // (it can Continue from there) — no reset to a fresh run.
    expect(runner.reset).not.toHaveBeenCalled();
    expect(provider.recordedChoices).toEqual([]);
    expect(snap().status).toBe("done");
    expect(snap().transcript).toEqual(["stuck"]);
    expect(notify).toHaveBeenCalledWith(DIVERGED_NOTIFICATION);
  });

  it("replays a valid log: applies the saved choice without truncating or notifying", () => {
    const runner = fullRunner({
      continueStory: (): Line[] => [
        { type: "text", text: "intro\n", tags: [] },
        { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Go", tags: [] }] },
      ],
    });
    const { provider, notify, snap } = startWithSavedLog(runner, [0]);

    // The single saved choice is applied; the log isn't discarded and no
    // divergence notification fires.
    expect(runner.choose).toHaveBeenCalledWith(0);
    expect(runner.reset).not.toHaveBeenCalled();
    expect(snap().transcript).toContain("> Go");
    expect(provider.recordedChoices).toEqual([0]);
    expect(notify).not.toHaveBeenCalled();
  });

  it("truncates and notifies when the story ends before consuming all saved choices", () => {
    let pass = 0;
    const runner = fullRunner({
      continueStory: (): Line[] => {
        pass += 1;
        if (pass === 1) {
          return [
            { type: "text", text: "intro\n", tags: [] },
            { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Go", tags: [] }] },
          ];
        }
        return [
          { type: "text", text: "the end\n", tags: [] },
          { type: "end", text: "", tags: [] },
        ];
      },
    });
    // Two saved choices, but the story ends after the first.
    const { provider, notify, snap } = startWithSavedLog(runner, [0, 1]);

    expect(runner.choose).toHaveBeenCalledTimes(1);
    expect(snap().status).toBe("ended");
    expect(provider.recordedChoices).toEqual([0]); // truncated to what was consumed
    expect(notify).toHaveBeenCalledWith(DIVERGED_NOTIFICATION);
    // The persisted log matches the truncation.
    expect(JSON.parse(localStorage.getItem(SAVE_KEY)!)).toEqual({ choiceLog: [0] });
  });

  it("truncates at the choice point when a saved choice index is no longer offered", () => {
    const offered = [{ index: 0, text: "Only option", tags: [] }];
    const runner = fullRunner({
      continueStory: (): Line[] => [
        { type: "choices", text: "", tags: [], choices: offered },
      ],
      // choosing an index the story doesn't offer would throw in the real
      // runtime — replay must not even attempt it.
      choose: vi.fn((i: number) => {
        if (i !== 0) throw new Error("invalid choice");
      }),
    });
    const { provider, notify, snap } = startWithSavedLog(runner, [5]); // 5 no longer exists

    // Divergence: stay at the choice point with what is offered now, instead
    // of resetting to a fresh run.
    expect(runner.choose).not.toHaveBeenCalled();
    expect(runner.reset).not.toHaveBeenCalled();
    expect(provider.recordedChoices).toEqual([]);
    expect(snap().status).toBe("awaiting-choice");
    expect(snap().choices).toEqual(offered);
    expect(notify).toHaveBeenCalledWith(DIVERGED_NOTIFICATION);
  });

  it("truncates with an error status when the runtime throws mid-replay", () => {
    let pass = 0;
    const runner = fullRunner({
      continueStory: (): Line[] => {
        pass += 1;
        if (pass === 1) {
          return [
            { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Go", tags: [] }] },
          ];
        }
        throw new Error("vm exploded");
      },
    });
    const { provider, notify, snap } = startWithSavedLog(runner, [0, 1]);

    expect(snap().status).toBe("error");
    expect(provider.recordedChoices).toEqual([0]);
    expect(snap().transcript.join("\n")).toContain("vm exploded");
    expect(notify).toHaveBeenCalledWith(DIVERGED_NOTIFICATION);
  });
});
