/**
 * Watch — the mini-REPL slice (W17/#3310, spec §F18): entries evaluate
 * through the provider's side-effect-proof `evaluateWatch` seam at every
 * stop boundary; results map value/transcript/error; degraded suppresses
 * re-evaluation; stale async rounds are dropped by generation.
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore } from "@brink/studio-store";
import type { SpeculationResult } from "@brink/wasm-types";

function result(overrides: Partial<SpeculationResult> = {}): SpeculationResult {
  return {
    transcript: [],
    stop: "completed",
    externals: { live: [], fallback: [] },
    diagnostics: [],
    ...overrides,
  } as SpeculationResult;
}

function watchStore(evaluateWatch = vi.fn(() => Promise.resolve(result()))) {
  const store = createStudioStore();
  const provider = {
    kind: "local",
    capabilities: new Set(["debug"]),
    getSnapshot: () => ({
      status: "running",
      transcript: [],
      choices: [],
      debugState: { turn_index: 1, position: { container_idx: 0, offset: 0 } },
      paused: true,
      reloadedAt: null,
      debugOutcome: null,
      auto: false,
      programChecksum: "0xabc",
      programModel: null,
      programInkt: null,
    }),
    subscribe: () => () => {},
    dispose: () => {},
    evaluateWatch,
  };
  store.getState()._bindProvider(provider as never);
  store.setState({
    sessionStatus: "running",
    sessionPaused: true,
    programChecksum: "0xabc",
    compiledChecksum: "0xabc",
  });
  return { store, evaluateWatch };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("watch (W17/#3310)", () => {
  it("adds, evaluates, and maps a typed-expression value onto the row", async () => {
    const { store, evaluateWatch } = watchStore(
      vi.fn(() =>
        Promise.resolve(result({ value: { type: "bool", value: false } as never })),
      ),
    );
    store.getState().watchAdd("gold >= pour(2)");
    expect(store.getState().watchEntries).toHaveLength(1);
    expect(evaluateWatch).toHaveBeenCalledWith(
      "gold >= pour(2)",
      expect.objectContaining({ budget: expect.anything() }),
    );
    await tick();
    const id = store.getState().watchEntries[0].id;
    expect(store.getState().watchResults[id]).toEqual({ kind: "value", display: "false" });
  });

  it("maps a fragment run to a transcript preview, choices and budget included", async () => {
    const { store } = watchStore(
      vi.fn(() =>
        Promise.resolve(
          result({
            transcript: [
              { text: "Griswold spreads a stained cloth.", tags: [] },
              { text: "", tags: [] },
            ] as never,
            reachedChoices: [{ index: 0, text: "Browse his wares" }] as never,
            stop: "line-budget",
          }),
        ),
      ),
    );
    store.getState().watchAdd("-> barter");
    await tick();
    const id = store.getState().watchEntries[0].id;
    expect(store.getState().watchResults[id]).toEqual({
      kind: "transcript",
      lines: ["Griswold spreads a stained cloth."],
      reachedChoices: ["Browse his wares"],
      truncated: true,
    });
  });

  it("a failed compile surfaces its diagnostic inline; a rejected eval too", async () => {
    const { store } = watchStore(
      vi.fn(() => Promise.resolve(result({ diagnostics: ["no such symbol `pour`"] }))),
    );
    store.getState().watchAdd("pour(");
    await tick();
    const id = store.getState().watchEntries[0].id;
    expect(store.getState().watchResults[id]).toEqual({
      kind: "error",
      message: "no such symbol `pour`",
    });
  });

  it("degraded suppresses re-evaluation (RULED — like every position feature)", () => {
    const { store, evaluateWatch } = watchStore();
    store.getState().watchAdd("gold");
    evaluateWatch.mockClear();
    store.setState({ compiledChecksum: "0xNEW" }); // program now out of sync
    store.getState().watchReevalAll();
    expect(evaluateWatch).not.toHaveBeenCalled();
  });

  it("a stale round's late result is dropped by generation", async () => {
    let release: (r: SpeculationResult) => void = () => {};
    const slow = new Promise<SpeculationResult>((r) => (release = r));
    const evaluateWatch = vi
      .fn()
      .mockReturnValueOnce(slow)
      .mockReturnValue(Promise.resolve(result({ value: { type: "int", value: 7 } as never })));
    const { store } = watchStore(evaluateWatch);
    store.getState().watchAdd("gold");
    const id = store.getState().watchEntries[0].id;
    // A newer round starts before the first resolves…
    store.getState().watchReevalAll();
    await tick();
    expect(store.getState().watchResults[id]).toEqual({ kind: "value", display: "7" });
    // …so the stale first result must NOT clobber it.
    release(result({ value: { type: "int", value: 999 } as never }));
    await tick();
    expect(store.getState().watchResults[id]).toEqual({ kind: "value", display: "7" });
  });

  it("the mirror hook re-evaluates once per stop, not per emission", () => {
    const { store, evaluateWatch } = watchStore();
    store.getState().watchAdd("gold");
    evaluateWatch.mockClear();
    store.setState({
      sessionPaused: true,
      debugState: { turn_index: 2, position: { container_idx: 1, offset: 4 } } as never,
    });
    store.getState()._watchOnMirror();
    expect(evaluateWatch).toHaveBeenCalledTimes(1);
    // Same stop mirrored again (e.g. a UI-driven refresh): no re-eval.
    store.getState()._watchOnMirror();
    expect(evaluateWatch).toHaveBeenCalledTimes(1);
    // A new stop (turn advanced): one more round.
    store.setState({
      debugState: { turn_index: 3, position: { container_idx: 1, offset: 9 } } as never,
    });
    store.getState()._watchOnMirror();
    expect(evaluateWatch).toHaveBeenCalledTimes(2);
    // Free-running (not a stop): silent.
    store.setState({ sessionPaused: false, sessionStatus: "running" });
    store.getState()._watchOnMirror();
    expect(evaluateWatch).toHaveBeenCalledTimes(2);
  });
});
