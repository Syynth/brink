import { describe, it, expect, vi } from "vitest";
import { createStudioStore, sessionCanContinue } from "@brink/studio-store";

type Line = {
  type: string;
  text: string;
  tags: string[];
  choices?: { index: number; text: string; tags: string[] }[];
};

function storeWithRunner(line: Line) {
  const store = createStudioStore();
  store.setState({
    _runner: { continueSingle: () => line, reset: vi.fn(), choose: vi.fn() } as never,
    sessionStatus: "running",
  });
  return store;
}

describe("session revealNext", () => {
  it("keeps the Continue affordance on a Done line (#6)", () => {
    // `-> DONE` is a turn boundary, not the end — the player must be able to resume.
    const store = storeWithRunner({ type: "done", text: "the turn ends\n", tags: [] });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("done");
    expect(sessionCanContinue(s.sessionStatus)).toBe(true);
    expect(s.sessionChoices).toEqual([]);
    expect(s.sessionText).toContain("the turn ends");
  });

  it("stays running (Continue) on a Text line", () => {
    const store = storeWithRunner({ type: "text", text: "more...\n", tags: [] });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("running");
    expect(sessionCanContinue(s.sessionStatus)).toBe(true);
  });

  it("marks ended (no Continue) on an End line", () => {
    const store = storeWithRunner({ type: "end", text: "fin\n", tags: [] });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("ended");
    expect(sessionCanContinue(s.sessionStatus)).toBe(false);
  });

  it("surfaces choices (awaiting-choice, no Continue) on a Choices line", () => {
    const store = storeWithRunner({
      type: "choices",
      text: "",
      tags: [],
      choices: [{ index: 0, text: "Go", tags: [] }],
    });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("awaiting-choice");
    expect(sessionCanContinue(s.sessionStatus)).toBe(false);
    expect(s.sessionChoices).toHaveLength(1);
  });

  it("transitions to error when the runtime throws", () => {
    const store = createStudioStore();
    store.setState({
      _runner: {
        continueSingle: () => {
          throw new Error("boom");
        },
      } as never,
      sessionStatus: "running",
    });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("error");
    expect(sessionCanContinue(s.sessionStatus)).toBe(false);
    expect(s.sessionText.join("\n")).toContain("Runtime error: boom");
  });
});
