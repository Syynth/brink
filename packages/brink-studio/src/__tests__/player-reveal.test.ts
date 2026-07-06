import { describe, it, expect, vi } from "vitest";
import {
  createStudioStore,
  LocalSessionProvider,
  sessionCanContinue,
} from "@brink/studio-store";

type Line = {
  type: string;
  text: string;
  tags: string[];
  choices?: { index: number; text: string; tags: string[] }[];
};

/** A minimal `StorySessionHandle`-shaped fake — enough surface for
 * `LocalSessionProvider` to drive `revealNext`/`chooseOption`. */
function fakeSession(overrides: Record<string, unknown> = {}) {
  return {
    continueToPause: vi.fn((): Line[] => [{ type: "end", text: "", tags: [] }]),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    ...overrides,
  };
}

// revealNext drives the bound SessionProvider (#179): the store mirrors the
// provider's snapshot into the reactive fields the views read. We bind a
// LocalSessionProvider wrapping a minimal scripted session and assert on the
// mirrored store state — the session is a provider implementation detail.
function storeWithSession(session: Record<string, unknown>) {
  const store = createStudioStore();
  const provider = new LocalSessionProvider({
    session: session as never,
    status: "running",
  });
  store.getState()._bindProvider(provider);
  return store;
}

function storeRevealing(line: Line) {
  return storeWithSession(fakeSession({ continueToPause: () => [line] }));
}

describe("session revealNext", () => {
  it("keeps the Continue affordance on a Done line (#6)", () => {
    // `-> DONE` is a turn boundary, not the end — the player must be able to resume.
    const store = storeRevealing({ type: "done", text: "the turn ends\n", tags: [] });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("done");
    expect(sessionCanContinue(s.sessionStatus)).toBe(true);
    expect(s.sessionChoices).toEqual([]);
    expect(s.sessionText).toContain("the turn ends");
  });

  it("stays running (Continue) on a Text line", () => {
    const store = storeRevealing({ type: "text", text: "more...\n", tags: [] });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("running");
    expect(sessionCanContinue(s.sessionStatus)).toBe(true);
  });

  it("marks ended (no Continue) on an End line", () => {
    const store = storeRevealing({ type: "end", text: "fin\n", tags: [] });
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("ended");
    expect(sessionCanContinue(s.sessionStatus)).toBe(false);
  });

  it("surfaces choices (awaiting-choice, no Continue) on a Choices line", () => {
    const store = storeRevealing({
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
    const store = storeWithSession(
      fakeSession({
        continueToPause: () => {
          throw new Error("boom");
        },
      }),
    );
    store.getState().revealNext();
    const s = store.getState();
    expect(s.sessionStatus).toBe("error");
    expect(sessionCanContinue(s.sessionStatus)).toBe(false);
    expect(s.sessionText.join("\n")).toContain("Runtime error: boom");
  });
});
