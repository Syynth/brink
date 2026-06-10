import { describe, it, expect, vi, beforeEach } from "vitest";
import { replayChoices, REPLAY_DIVERGED_MESSAGE } from "@brink/studio-store";

// replayChoices fast-forwards a recorded choice log on session start. Two
// behaviors are pinned here:
//
// 1. Termination: before the guard, a story that reaches a `-> DONE` dead-end
//    before the next saved choice made continueStory() return a
//    non-choice/non-end pass, so the while loop spun forever and locked the
//    UI thread (#7).
// 2. Divergence (spec §7.6): when a recorded choice no longer applies, the
//    history is *truncated at the divergence point* (not discarded wholesale),
//    the session stays at the position it reached, and the interim Toast
//    carries the divergence notification.

type Line = { type: string; text: string; tags: string[]; choices?: { index: number; text: string; tags: string[] }[] };

function makeHarness(runner: Record<string, unknown>, choiceLog: number[]) {
  let state: Record<string, unknown> = {
    _runner: runner,
    _choiceLog: choiceLog,
    toastMessage: null,
    revealNext: vi.fn(),
    _refreshDebugState: vi.fn(),
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

  it("terminates and truncates when the story dead-ends on DONE before a saved choice", () => {
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

    expect(runner.choose).not.toHaveBeenCalled();
    // Divergence at choice 0: nothing kept, session stays at the DONE
    // boundary (it can Continue from there) — no reset to a fresh run.
    expect(runner.reset).not.toHaveBeenCalled();
    expect(h.getState()._choiceLog).toEqual([]);
    expect(h.getState().sessionStatus).toBe("done");
    expect(h.getState().sessionText).toEqual(["stuck"]);
    expect(h.getState().toastMessage).toBe(REPLAY_DIVERGED_MESSAGE);
  });

  it("replays a valid log: applies the saved choice without truncating or notifying", () => {
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

    // The single saved choice is applied; the log isn't discarded, no
    // divergence toast fires, and the player resumes via revealNext().
    expect(runner.choose).toHaveBeenCalledWith(0);
    expect(runner.reset).not.toHaveBeenCalled();
    expect(h.getState().sessionText).toContain("> Go");
    expect(h.getState()._choiceLog).toEqual([0]);
    expect(h.getState().toastMessage).toBeNull();
    expect((h.getState().revealNext as ReturnType<typeof vi.fn>)).toHaveBeenCalled();
  });

  it("truncates and notifies when the story ends before consuming all saved choices", () => {
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
    expect(h.getState().sessionStatus).toBe("ended");
    expect(h.getState()._choiceLog).toEqual([0]); // truncated to what was consumed
    expect(h.getState().toastMessage).toBe(REPLAY_DIVERGED_MESSAGE);
    // The persisted log matches the truncation.
    expect(JSON.parse(localStorage.getItem("brink-player-save")!)).toEqual({ choiceLog: [0] });
  });

  it("truncates at the choice point when a saved choice index is no longer offered", () => {
    const offered = [{ index: 0, text: "Only option", tags: [] }];
    const runner = {
      continueStory: (): Line[] => [
        { type: "choices", text: "", tags: [], choices: offered },
      ],
      reset: vi.fn(),
      // choosing an index the story doesn't offer would throw in the real
      // runtime — replay must not even attempt it.
      choose: vi.fn((i: number) => {
        if (i !== 0) throw new Error("invalid choice");
      }),
    };
    const h = makeHarness(runner, [5]); // saved choice 5 no longer exists

    replayChoices(h.set as never, h.get as never, [5]);

    // Divergence: stay at the choice point with what is offered now, instead
    // of resetting to a fresh run.
    expect(runner.choose).not.toHaveBeenCalled();
    expect(runner.reset).not.toHaveBeenCalled();
    expect(h.getState()._choiceLog).toEqual([]);
    expect(h.getState().sessionStatus).toBe("awaiting-choice");
    expect(h.getState().sessionChoices).toEqual(offered);
    expect(h.getState().toastMessage).toBe(REPLAY_DIVERGED_MESSAGE);
  });

  it("truncates with an error status when the runtime throws mid-replay", () => {
    let pass = 0;
    const runner = {
      continueStory: (): Line[] => {
        pass += 1;
        if (pass === 1) {
          return [
            { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Go", tags: [] }] },
          ];
        }
        throw new Error("vm exploded");
      },
      reset: vi.fn(),
      choose: vi.fn(),
    };
    const h = makeHarness(runner, [0, 1]);

    replayChoices(h.set as never, h.get as never, [0, 1]);

    expect(h.getState().sessionStatus).toBe("error");
    expect(h.getState()._choiceLog).toEqual([0]);
    expect((h.getState().sessionText as string[]).join("\n")).toContain("vm exploded");
    expect(h.getState().toastMessage).toBe(REPLAY_DIVERGED_MESSAGE);
  });
});
