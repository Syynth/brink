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
    // `reveal()` calls `continueSingle` by default and `continueToPause` only
    // under `auto` (#3011), so the fake supplies both.
    continueSingle: vi.fn((): Line => ({ type: "end", text: "", tags: [] })),
    continueToPause: vi.fn((): Line[] => [{ type: "end", text: "", tags: [] }]),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
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
  return storeWithSession(
    fakeSession({ continueSingle: () => line, continueToPause: () => [line] }),
  );
}

describe("reveal mode (#3011)", () => {
  // The defect: `reveal()` called `continueToPause()` unconditionally, so one
  // Continue press dumped every line to the next choice. These pin WHICH verb
  // each mode uses — asserting only on transcript length would pass against a
  // maximal call that happened to return one line.

  it("advances ONE line by default — continueSingle, never continueToPause", () => {
    const session = fakeSession({
      continueSingle: vi.fn((): Line => ({ type: "text", text: "one\n", tags: [] })),
      continueToPause: vi.fn((): Line[] => [
        { type: "text", text: "one\n", tags: [] },
        { type: "text", text: "two\n", tags: [] },
        { type: "end", text: "", tags: [] },
      ]),
    });
    const store = storeWithSession(session);
    store.getState().revealNext();

    expect(session.continueSingle).toHaveBeenCalledTimes(1);
    expect(session.continueToPause).not.toHaveBeenCalled();
    expect(store.getState().sessionText).toEqual(["one"]);
  });

  it("runs to the next pause once auto is on", () => {
    const session = fakeSession({
      continueSingle: vi.fn((): Line => ({ type: "text", text: "one\n", tags: [] })),
      continueToPause: vi.fn((): Line[] => [
        { type: "text", text: "one\n", tags: [] },
        { type: "text", text: "two\n", tags: [] },
        { type: "end", text: "", tags: [] },
      ]),
    });
    const store = storeWithSession(session);
    store.getState().setSessionAuto(true);
    // This pin is about the BATCH road ("all at once") — the W7 paced
    // default (F13) would pump line-by-line instead; see
    // paced-reveal.test.ts for that mode's own pins.
    store.getState().setSessionPaced(0);
    store.getState().revealNext();

    expect(session.continueToPause).toHaveBeenCalledTimes(1);
    expect(session.continueSingle).not.toHaveBeenCalled();
    expect(store.getState().sessionText).toEqual(["one", "two"]);
  });

  it("defaults to off, and mirrors the mode into the store", () => {
    const store = storeWithSession(fakeSession());
    expect(store.getState().sessionAuto).toBe(false);

    store.getState().setSessionAuto(true);
    expect(store.getState().sessionAuto).toBe(true);

    store.getState().setSessionAuto(false);
    expect(store.getState().sessionAuto).toBe(false);
  });

  it("does not retroactively change what is already revealed", () => {
    // Flipping the mode mid-scene affects the NEXT reveal only — it must not
    // replay or collapse the transcript the author is already reading.
    const session = fakeSession({
      continueSingle: vi.fn((): Line => ({ type: "text", text: "first\n", tags: [] })),
      continueToPause: vi.fn((): Line[] => [{ type: "text", text: "rest\n", tags: [] }]),
    });
    const store = storeWithSession(session);
    store.getState().revealNext();
    expect(store.getState().sessionText).toEqual(["first"]);

    store.getState().setSessionAuto(true);
    expect(store.getState().sessionText).toEqual(["first"]);
  });
});

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
        continueSingle: () => {
          throw new Error("boom");
        },
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
