import { describe, it, expect, vi, beforeEach } from "vitest";
import { LocalSessionProvider, REPLAY_DIVERGED_MESSAGE } from "@brink/studio-store";

// The LocalSessionProvider fast-forwards a persisted, pre-#388 legacy choice
// log ({choiceLog: number[]}) via a one-time migration the first time it
// starts a fresh session (docs/story-session-spec.md's migration ruling):
// replay it against the session exactly like the old `replayWalk` did, but
// building a real journal along the way. Driving `start()` with a seeded
// legacy save exercises the real entry point. Two behaviors are pinned:
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

/** A scripted `StorySessionHandle`-shaped fake with the surface the provider
 * drives during migration/replay. */
function fullSession(overrides: Record<string, unknown>): Record<string, unknown> {
  return {
    continueToPause: (): Line[] => [{ type: "end", text: "", tags: [] }],
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    exportJournal: vi.fn(() => ({ version: 1, program_checksum: 0, events: [], truncated: false })),
    ...overrides,
  };
}

/** Construct a provider whose fresh session (built by `start()`, no
 * adoption) is the scripted fake, seed the legacy saved log, and start
 * (triggers the one-time migration). Uses the `sessionFactory` test seam so
 * `start()` takes its real "no live session yet" branch — the one legacy
 * migration is actually reachable from — instead of the hot-reload branch a
 * pre-adopted session would trigger. */
function startWithSavedLog(session: Record<string, unknown>, choiceLog: number[]) {
  const notify = vi.fn();
  localStorage.setItem(SAVE_KEY, JSON.stringify({ choiceLog }));
  const provider = new LocalSessionProvider({
    sessionFactory: () => session as never,
  });
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

describe("LocalSessionProvider legacy choice-log migration", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("terminates and truncates when the story dead-ends on DONE before a saved choice", () => {
    // Always returns text + a DONE terminal, never a choice or end.
    const session = fullSession({
      continueToPause: (): Line[] => [
        { type: "text", text: "stuck\n", tags: [] },
        { type: "done", text: "", tags: [] },
      ],
    });
    const { notify, snap } = startWithSavedLog(session, [0, 1]);

    // Without the guard this never returns. The iteration cap + consumedChoice
    // bail guarantee it does.
    expect(session.choose).not.toHaveBeenCalled();
    // Divergence at choice 0: session stays at the DONE boundary (it can
    // Continue from there) — no reset to a fresh run.
    expect(session.restart).not.toHaveBeenCalled();
    expect(snap().status).toBe("done");
    expect(snap().transcript.map((l) => l.text)).toEqual(["stuck"]);
    expect(notify).toHaveBeenCalledWith(DIVERGED_NOTIFICATION);
  });

  it("replays a valid log: applies the saved choice without truncating or notifying", () => {
    const session = fullSession({
      continueToPause: (): Line[] => [
        { type: "text", text: "intro\n", tags: [] },
        { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Go", tags: [] }] },
      ],
    });
    const { notify, snap } = startWithSavedLog(session, [0]);

    // The single saved choice is applied; no divergence notification fires.
    expect(session.choose).toHaveBeenCalledWith(0);
    expect(session.restart).not.toHaveBeenCalled();
    expect(snap().transcript.map((l) => l.text)).toContain("> Go");
    expect(notify).not.toHaveBeenCalled();
  });

  it("truncates and notifies when the story ends before consuming all saved choices", () => {
    let pass = 0;
    const session = fullSession({
      continueToPause: (): Line[] => {
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
    const { notify, snap } = startWithSavedLog(session, [0, 1]);

    expect(session.choose).toHaveBeenCalledTimes(1);
    expect(snap().status).toBe("ended");
    expect(notify).toHaveBeenCalledWith(DIVERGED_NOTIFICATION);
    // The migrated journal (new format) is what's persisted now — the legacy
    // blob is gone.
    const persisted = JSON.parse(localStorage.getItem(SAVE_KEY)!);
    expect(persisted.version).toBe(2);
  });

  it("truncates at the choice point when a saved choice index is no longer offered", () => {
    const offered = [{ index: 0, text: "Only option", tags: [] }];
    const session = fullSession({
      continueToPause: (): Line[] => [
        { type: "choices", text: "", tags: [], choices: offered },
      ],
      // choosing an index the story doesn't offer would throw in the real
      // runtime — migration must not even attempt it.
      choose: vi.fn((i: number) => {
        if (i !== 0) throw new Error("invalid choice");
      }),
    });
    const { notify, snap } = startWithSavedLog(session, [5]); // 5 no longer exists

    // Divergence: stay at the choice point with what is offered now, instead
    // of resetting to a fresh run.
    expect(session.choose).not.toHaveBeenCalled();
    expect(session.restart).not.toHaveBeenCalled();
    expect(snap().status).toBe("awaiting-choice");
    expect(snap().choices).toEqual(offered);
    expect(notify).toHaveBeenCalledWith(DIVERGED_NOTIFICATION);
  });

  it("truncates with an error status when the runtime throws mid-replay", () => {
    let pass = 0;
    const session = fullSession({
      continueToPause: (): Line[] => {
        pass += 1;
        if (pass === 1) {
          return [
            { type: "choices", text: "", tags: [], choices: [{ index: 0, text: "Go", tags: [] }] },
          ];
        }
        throw new Error("vm exploded");
      },
    });
    const { notify, snap } = startWithSavedLog(session, [0, 1]);

    expect(snap().status).toBe("error");
    expect(snap().transcript.map((l) => l.text).join("\n")).toContain("vm exploded");
    expect(notify).toHaveBeenCalledWith(DIVERGED_NOTIFICATION);
  });
});
