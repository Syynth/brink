import { describe, it, expect, vi, beforeEach } from "vitest";
import { replayChoices } from "@brink/studio-store";

// replayChoices fast-forwards a saved choice log on load. Before the fix, a
// story that reaches a `-> DONE` dead-end before the next saved choice made
// continueStory() return a non-choice/non-end pass, so the while loop spun
// forever and locked the UI thread (#7). These tests pin the termination guard.

type Line = { type: string; text: string; tags: string[]; choices?: { index: number; text: string; tags: string[] }[] };

function makeHarness(runner: Record<string, unknown>, choiceLog: number[]) {
  let state: Record<string, unknown> = {
    _runner: runner,
    _choiceLog: choiceLog,
    revealNext: vi.fn(),
  };
  const get = () => state as never;
  const set = (partial: unknown) => {
    const next = typeof partial === "function" ? (partial as (s: unknown) => object)(state) : partial;
    state = { ...state, ...(next as object) };
  };
  return { get, set, getState: () => state };
}

describe("replayChoices", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("terminates and resets when the story dead-ends on DONE before a saved choice", () => {
    // Always returns text + a DONE terminal, never a choice or end.
    const runner = {
      continueStory: (): Line[] => [
        { type: "text", text: "stuck\n", tags: [] },
        { type: "done", text: "", tags: [] },
      ],
      reset: vi.fn(),
      choose: vi.fn(),
    };
    const h = makeHarness(runner, [0, 1]);

    // Without the guard this never returns. The iteration cap + consumedChoice
    // bail guarantee it does.
    replayChoices(h.set as never, h.get as never, [0, 1]);

    expect(runner.reset).toHaveBeenCalled();
    expect(runner.choose).not.toHaveBeenCalled();
    expect(h.getState()._choiceLog).toEqual([]);
    expect((h.getState().revealNext as ReturnType<typeof vi.fn>)).toHaveBeenCalled();
  });

  it("replays a valid log: applies the saved choice without bailing", () => {
    const runner = {
      continueStory: (): Line[] => [
        { type: "text", text: "intro\n", tags: [] },
        { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Go", tags: [] }] },
      ],
      reset: vi.fn(),
      choose: vi.fn(),
    };
    const h = makeHarness(runner, [0]);

    replayChoices(h.set as never, h.get as never, [0]);

    // The single saved choice is applied; the log isn't discarded, and the
    // player resumes via revealNext() after the replay.
    expect(runner.choose).toHaveBeenCalledWith(0);
    expect(runner.reset).not.toHaveBeenCalled();
    expect(h.getState().playerText).toContain("> Go");
    expect((h.getState().revealNext as ReturnType<typeof vi.fn>)).toHaveBeenCalled();
  });

  it("ends during replay when the story ends before consuming all saved choices", () => {
    let pass = 0;
    const runner = {
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
      reset: vi.fn(),
      choose: vi.fn(),
    };
    // Two saved choices, but the story ends after the first.
    const h = makeHarness(runner, [0, 1]);

    replayChoices(h.set as never, h.get as never, [0, 1]);

    expect(runner.choose).toHaveBeenCalledTimes(1);
    expect(h.getState().playerEnded).toBe(true);
    expect(h.getState()._choiceLog).toEqual([0]); // truncated to what was consumed
  });

  it("bails when a saved choice index is no longer offered", () => {
    const runner = {
      continueStory: (): Line[] => [
        { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Only option", tags: [] }] },
      ],
      reset: vi.fn(),
      // choosing an index the story doesn't offer throws, like the real runtime
      choose: vi.fn((i: number) => {
        if (i !== 0) throw new Error("invalid choice");
      }),
    };
    const h = makeHarness(runner, [5]); // saved choice 5 no longer exists

    replayChoices(h.set as never, h.get as never, [5]);

    expect(runner.reset).toHaveBeenCalled();
    expect(h.getState()._choiceLog).toEqual([]);
  });
});
