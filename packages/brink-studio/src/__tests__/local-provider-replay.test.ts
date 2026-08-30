/**
 * `LocalSessionProvider` reload/restore replay-outcome coverage
 * (docs/story-session-spec.md §7.6, #388/#401).
 *
 * These paths were previously exercised only by e2e:
 *
 * 1. `applyReplayOutcome`'s `diverged` and `failed` branches, driven through
 *    `start()`'s hot-reload path (a live `prev` session whose `reload()`
 *    returns each `ReplayOutcome` variant).
 * 2. `restoreFromJournal` — the `StorySessionHandle.restore()` path taken on
 *    a fresh `start()` when a v2 journal save is present.
 * 3. `choicesFromDebugState` — pending choices must be built from the raw
 *    `DebugChoice.index` (pre-filter `pending_choices` position), not the
 *    post-filter array position, or `choose()` dispatches the wrong index
 *    whenever an invisible-default choice is mixed in at the same pause.
 * 4. The `reload()`-throws recovery path: a hot-reload whose `reload()`
 *    rejects (decode/link failure of the recompiled bytes) must free the
 *    stale session and fall back to a fresh one instead of leaking the wasm
 *    handle and dead-ending in a permanent error state.
 */

import { describe, it, expect, vi } from "vitest";
import { LocalSessionProvider, REPLAY_DIVERGED_MESSAGE } from "@brink/studio-store";
import { StorySessionHandle } from "@brink-lang/web";
import type { DebugState, ReplayOutcome } from "@brink/wasm-types";

/** A scripted `StorySessionHandle`-shaped fake — the surface `start()`/
 * `reload()`/choice-driving code touches. `reload` is overridable per test to
 * script each `ReplayOutcome` variant (or throw). */
function fakeSession(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    continueSingle: vi.fn(() => ({ type: "end", text: "", tags: [] })),
    continueToPause: vi.fn(() => [{ type: "end", text: "", tags: [] }]),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    exportJournal: vi.fn(() => ({ version: 1, program_checksum: 0, events: [], truncated: false })),
    programModel: vi.fn(() => ({ checksum: "0xabc", globals: [], lists: [], externals: [], knots: [] })),
    programInkt: vi.fn(() => ""),
    reload: vi.fn((): ReplayOutcome => ({ type: "replayed", warnings: [] })),
    ...overrides,
  };
}

/** Build a provider that already has `session` bound as its live runner
 * (models "the studio has a running session"), so `start()` takes the
 * hot-reload branch. */
function providerWithLiveSession(session: Record<string, unknown>): LocalSessionProvider {
  const provider = new LocalSessionProvider({ session: session as never });
  provider.setCallbacks({ notify: vi.fn(), appendOutput: vi.fn() });
  return provider;
}

const NEW_BYTES = new Uint8Array([9, 9, 9]);

describe("LocalSessionProvider.start() hot-reload replay outcomes", () => {
  it("'replayed': no divergence notification, status/choices mirror the debug snapshot", () => {
    const debugState: DebugState = {
      status: "waiting_for_choice",
      turn_index: 3,
      globals: [],
      call_stack: [],
      visit_counts: [],
      pending_choices: [{ text: "Go north", index: 0 }, { text: "Go south", index: 1 }],
      rng: { seed: 0, previous: 0 },
    };
    const session = fakeSession({
      reload: vi.fn((): ReplayOutcome => ({ type: "replayed", warnings: [] })),
      debugSnapshot: vi.fn(() => debugState),
    });
    const notify = vi.fn();
    const provider = new LocalSessionProvider({ session: session as never });
    provider.setCallbacks({ notify, appendOutput: vi.fn() });

    provider.start(NEW_BYTES);

    expect(session.reload).toHaveBeenCalledWith(NEW_BYTES);
    expect(notify).not.toHaveBeenCalled();
    const snap = provider.getSnapshot();
    expect(snap.status).toBe("awaiting-choice");
    expect(snap.choices).toEqual([
      { index: 0, text: "Go north", tags: [] },
      { index: 1, text: "Go south", tags: [] },
    ]);
  });

  it("'diverged': notifies with the standard divergence warning, keeps running status", () => {
    const debugState: DebugState = {
      status: "active",
      turn_index: 1,
      globals: [],
      call_stack: [],
      visit_counts: [],
      pending_choices: [],
      rng: { seed: 0, previous: 0 },
    };
    const outcome: ReplayOutcome = {
      type: "diverged",
      at_event: 2,
      expected: { kind: { type: "choice", index: 0 } },
      found: { type: "not_waiting_for_choice" },
    };
    const session = fakeSession({
      reload: vi.fn(() => outcome),
      debugSnapshot: vi.fn(() => debugState),
    });
    const notify = vi.fn();
    const provider = providerWithLiveSession(session);
    provider.setCallbacks({ notify, appendOutput: vi.fn() });

    provider.start(NEW_BYTES);

    expect(notify).toHaveBeenCalledWith({
      severity: "warning",
      source: "story",
      message: REPLAY_DIVERGED_MESSAGE,
    });
    expect(provider.getSnapshot().status).toBe("running");
  });

  it("'failed' with runtime_error: appends the error to transcript, sets status error, still notifies diverged", () => {
    const debugState: DebugState = {
      status: "ended",
      turn_index: 5,
      globals: [],
      call_stack: [],
      visit_counts: [],
      pending_choices: [],
      rng: { seed: 0, previous: 0 },
    };
    const outcome: ReplayOutcome = {
      type: "failed",
      at_event: 4,
      reason: { type: "runtime_error", message: "stack underflow" },
    };
    const session = fakeSession({
      reload: vi.fn(() => outcome),
      debugSnapshot: vi.fn(() => debugState),
    });
    const notify = vi.fn();
    const appendOutput = vi.fn();
    const provider = providerWithLiveSession(session);
    provider.setCallbacks({ notify, appendOutput });

    provider.start(NEW_BYTES);

    const snap = provider.getSnapshot();
    expect(snap.status).toBe("error");
    expect(snap.transcript.map((l) => l.text).join("\n")).toContain("stack underflow");
    expect(appendOutput).toHaveBeenCalledWith("story", expect.stringContaining("stack underflow"));
    expect(notify).toHaveBeenCalledWith({
      severity: "warning",
      source: "story",
      message: REPLAY_DIVERGED_MESSAGE,
    });
  });

  it("'failed' with budget/awaiting_external: notifies diverged without touching the transcript", () => {
    const debugState: DebugState = {
      status: "active",
      turn_index: 5,
      globals: [],
      call_stack: [],
      visit_counts: [],
      pending_choices: [],
      rng: { seed: 0, previous: 0 },
    };
    const outcome: ReplayOutcome = {
      type: "failed",
      at_event: 4,
      reason: { type: "budget" },
    };
    const session = fakeSession({
      reload: vi.fn(() => outcome),
      debugSnapshot: vi.fn(() => debugState),
    });
    const notify = vi.fn();
    const provider = providerWithLiveSession(session);
    provider.setCallbacks({ notify, appendOutput: vi.fn() });

    provider.start(NEW_BYTES);

    expect(provider.getSnapshot().transcript).toEqual([]);
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ message: REPLAY_DIVERGED_MESSAGE }),
    );
  });
});

describe("LocalSessionProvider.start() hot-reload recovery when reload() throws", () => {
  it("frees the stale session and falls back to a fresh one on decode/link failure", () => {
    const prev = fakeSession({
      reload: vi.fn(() => {
        throw new Error("decode error: bad magic");
      }),
    });
    const fresh = fakeSession({
      continueSingle: vi.fn(() => ({ type: "text", text: "fresh start\n", tags: [] })),
      continueToPause: vi.fn(() => [{ type: "text", text: "fresh start\n", tags: [] }]),
    });
    const sessionFactory = vi.fn(() => fresh as never);
    const provider = new LocalSessionProvider({
      session: prev as never,
      sessionFactory,
    });
    provider.setCallbacks({ notify: vi.fn(), appendOutput: vi.fn() });

    provider.start(NEW_BYTES);

    // The stale session must be freed, not merely dropped (no leaked wasm handle).
    expect(prev.free).toHaveBeenCalledTimes(1);
    // A fresh session is constructed on the new bytes instead of dead-ending.
    expect(sessionFactory).toHaveBeenCalledWith(NEW_BYTES);
    const snap = provider.getSnapshot();
    expect(snap.status).not.toBe("error");
    expect(snap.transcript.map((l) => l.text)).toContain("fresh start");
  });

  it("does not double-free when the fallback session construction also throws", () => {
    const prev = fakeSession({
      reload: vi.fn(() => {
        throw new Error("decode error");
      }),
    });
    const sessionFactory = vi.fn(() => {
      throw new Error("also broken");
    });
    const provider = new LocalSessionProvider({
      session: prev as never,
      sessionFactory,
    });
    provider.setCallbacks({ notify: vi.fn(), appendOutput: vi.fn() });

    provider.start(NEW_BYTES);

    expect(prev.free).toHaveBeenCalledTimes(1);
    expect(provider.getSnapshot().status).toBe("error");
    expect(provider.getSnapshot().transcript.map((l) => l.text).join("\n")).toContain("also broken");
  });
});

describe("LocalSessionProvider choicesFromDebugState index reconstruction", () => {
  it("uses DebugChoice.index (raw pending_choices position) verbatim, not array position", () => {
    // Simulates a pause with an invisible-default choice at raw index 1: the
    // visible choices carry the *pre-filter* indices 0 and 2, skipping 1.
    const debugState: DebugState = {
      status: "waiting_for_choice",
      turn_index: 0,
      globals: [],
      call_stack: [],
      visit_counts: [],
      pending_choices: [
        { text: "First", index: 0 },
        { text: "Third", index: 2 },
      ],
      rng: { seed: 0, previous: 0 },
    };
    const session = fakeSession({
      reload: vi.fn((): ReplayOutcome => ({ type: "replayed", warnings: [] })),
      debugSnapshot: vi.fn(() => debugState),
    });
    const provider = providerWithLiveSession(session);
    provider.setCallbacks({ notify: vi.fn(), appendOutput: vi.fn() });

    provider.start(NEW_BYTES);

    const snap = provider.getSnapshot();
    expect(snap.choices).toEqual([
      { index: 0, text: "First", tags: [] },
      { index: 2, text: "Third", tags: [] },
    ]);

    // Choosing the second visible entry must dispatch the raw index 2, not
    // the array position 1 — otherwise it picks the wrong branch or throws.
    provider.choose(2);
    expect(session.choose).toHaveBeenCalledWith(2);
  });
});

describe("LocalSessionProvider.start() restoreFromJournal (v2 journal save)", () => {
  it("restores via StorySessionHandle.restore and applies the returned outcome", () => {
    const restoredDebugState: DebugState = {
      status: "waiting_for_choice",
      turn_index: 7,
      globals: [],
      call_stack: [],
      visit_counts: [],
      pending_choices: [{ text: "Continue on", index: 0 }],
      rng: { seed: 0, previous: 0 },
    };
    const restoredSession = fakeSession({
      debugSnapshot: vi.fn(() => restoredDebugState),
    });
    const restoreOutcome: ReplayOutcome = { type: "replayed", warnings: [] };
    const restoreSpy = vi
      .spyOn(StorySessionHandle, "restore")
      .mockReturnValue({ session: restoredSession as never, outcome: restoreOutcome });

    const journal = { version: 1, program_checksum: 0, events: [], truncated: false };
    localStorage.setItem(
      "brink-player-save",
      JSON.stringify({ version: 2, journal }),
    );

    const provider = new LocalSessionProvider({});
    provider.setCallbacks({ notify: vi.fn(), appendOutput: vi.fn() });

    provider.start(NEW_BYTES);

    expect(restoreSpy).toHaveBeenCalledWith(NEW_BYTES, journal);
    const snap = provider.getSnapshot();
    expect(snap.status).toBe("awaiting-choice");
    expect(snap.choices).toEqual([{ index: 0, text: "Continue on", tags: [] }]);

    restoreSpy.mockRestore();
    localStorage.clear();
  });

  it("notifies divergence when the restored journal diverges against the recompiled program", () => {
    const restoredDebugState: DebugState = {
      status: "active",
      turn_index: 2,
      globals: [],
      call_stack: [],
      visit_counts: [],
      pending_choices: [],
      rng: { seed: 0, previous: 0 },
    };
    const restoredSession = fakeSession({
      debugSnapshot: vi.fn(() => restoredDebugState),
    });
    const divergedOutcome: ReplayOutcome = {
      type: "diverged",
      at_event: 1,
      expected: { kind: { type: "choice", index: 0 } },
      found: { type: "choice_index_out_of_range", index: 0, available: 0 },
    };
    const restoreSpy = vi
      .spyOn(StorySessionHandle, "restore")
      .mockReturnValue({ session: restoredSession as never, outcome: divergedOutcome });

    const journal = { version: 1, program_checksum: 0, events: [], truncated: false };
    localStorage.setItem(
      "brink-player-save",
      JSON.stringify({ version: 2, journal }),
    );

    const notify = vi.fn();
    const provider = new LocalSessionProvider({});
    provider.setCallbacks({ notify, appendOutput: vi.fn() });

    provider.start(NEW_BYTES);

    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ message: REPLAY_DIVERGED_MESSAGE }),
    );

    restoreSpy.mockRestore();
    localStorage.clear();
  });
});
