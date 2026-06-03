import { describe, it, expect, vi } from "vitest";
import { createStudioStore } from "@brink/studio-store";

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
  });
  return store;
}

describe("player revealNext", () => {
  it("shows the Continue button on a Done line (#6)", () => {
    // `-> DONE` is a turn boundary, not the end — the player must be able to resume.
    const store = storeWithRunner({ type: "done", text: "the turn ends\n", tags: [] });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.playerCanContinue).toBe(true);
    expect(s.playerEnded).toBe(false);
    expect(s.playerChoices).toEqual([]);
    expect(s.playerText).toContain("the turn ends");
  });

  it("shows Continue on a Text line", () => {
    const store = storeWithRunner({ type: "text", text: "more...\n", tags: [] });
    store.getState().revealNext();
    expect(store.getState().playerCanContinue).toBe(true);
  });

  it("marks ended (no Continue) on an End line", () => {
    const store = storeWithRunner({ type: "end", text: "fin\n", tags: [] });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.playerEnded).toBe(true);
    expect(s.playerCanContinue).toBe(false);
  });

  it("surfaces choices (no Continue) on a Choices line", () => {
    const store = storeWithRunner({
      type: "choices",
      text: "",
      tags: [],
      choices: [{ index: 0, text: "Go", tags: [] }],
    });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.playerCanContinue).toBe(false);
    expect(s.playerChoices).toHaveLength(1);
  });
});
