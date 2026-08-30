/**
 * W5/#3298 — the unified drive loop, provider-routing half.
 *
 * The runtime truth (breakpoints only hit inside the debug loop; the
 * drained delivery cursor keeps one coherent transcript across roads) is
 * proven over a real WebSession in `crates/brink-web/src/session.rs`
 * (`interleaved_play_and_debug_keep_one_coherent_transcript`). What THIS
 * suite pins is the routing: which verb the provider drives when, how
 * outcome lines reach the transcript, and how paused-ness transitions.
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore, LocalSessionProvider } from "@brink/studio-store";
import type { Breakpoint, DebugRunOutcome } from "@brink/wasm-types";

type Line = { type: string; text: string; tags: string[] };

function outcome(
  reason: DebugRunOutcome["reason"],
  lines: { text: string; tags: string[] }[] = [],
): DebugRunOutcome {
  return { reason, depth: 1, lines };
}

function scriptedSession(armed: Breakpoint[] = []) {
  return {
    continueSingle: vi.fn((): Line => ({ type: "text", text: "prod line\n", tags: [] })),
    continueToPause: vi.fn((): Line[] => [{ type: "done", text: "", tags: [] }]),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    resolveDebugPosition: vi.fn(() => null),
    resolveSourceLine: vi.fn(() => null),
    hasDebugInfo: vi.fn(() => true),
    debugBreakpointAdd: vi.fn((): number => 0),
    debugBreakpointRemove: vi.fn((): boolean => true),
    debugBreakpointSetEnabled: vi.fn((): boolean => true),
    debugBreakpoints: vi.fn((): Breakpoint[] => armed),
    debugRun: vi.fn((): DebugRunOutcome => outcome({ type: "terminal" })),
    debugStep: vi.fn((): DebugRunOutcome => outcome({ type: "step" })),
    debugStepLine: vi.fn((): DebugRunOutcome => outcome({ type: "step" })),
  };
}

const BP: Breakpoint = { id: 1, container_idx: 0, offset: 4, name: "bp", enabled: true };

function bind(session: ReturnType<typeof scriptedSession>) {
  const store = createStudioStore();
  const provider = new LocalSessionProvider({
    session: session as never,
    status: "running",
  });
  store.getState()._bindProvider(provider);
  return { store, provider };
}

describe("unified drive loop (W5/#3298)", () => {
  it("with no breakpoints and not paused, reveal stays on the journaled road", () => {
    const session = scriptedSession();
    const { store } = bind(session);

    store.getState().revealNext();
    expect(session.continueSingle).toHaveBeenCalled();
    expect(session.debugStepLine).not.toHaveBeenCalled();
  });

  it("with breakpoints armed, a single reveal is a bounded line step", () => {
    const session = scriptedSession([BP]);
    session.debugStepLine.mockReturnValue(
      outcome({ type: "step" }, [{ text: "stepped line\n", tags: [] }]),
    );
    const { store } = bind(session);

    store.getState().revealNext();
    expect(session.debugStepLine).toHaveBeenCalledWith("over");
    expect(session.continueSingle).not.toHaveBeenCalled();
    // The outcome's lines delta reaches the transcript — the coherence
    // half: stepping over a text line must not eat its output.
    expect(store.getState().sessionText).toContain("stepped line");
    // An ordinary playing reveal does not enter the paused state.
    expect(store.getState().sessionPaused).toBe(false);
  });

  it("with breakpoints armed and auto on, reveal free-runs to the next stop", () => {
    const session = scriptedSession([BP]);
    const { store, provider } = bind(session);
    provider.setAuto(true);

    store.getState().revealNext();
    expect(session.debugRun).toHaveBeenCalled();
  });

  it("a breakpoint hit pauses the session; continue (debugRun) resumes", () => {
    const session = scriptedSession([BP]);
    session.debugStepLine.mockReturnValue(
      outcome({ type: "breakpoint", id: 1, name: "bp" }),
    );
    const { store } = bind(session);

    store.getState().revealNext();
    expect(store.getState().sessionPaused).toBe(true);

    // Continue: free-run that ends at a terminal — paused clears.
    session.debugRun.mockReturnValue(outcome({ type: "terminal" }));
    store.getState().debugRun();
    expect(store.getState().sessionPaused).toBe(false);
  });

  it("the pause verb pauses; a paused reveal line-steps and stays paused", () => {
    const session = scriptedSession(); // no breakpoints at all
    const { store } = bind(session);

    store.getState().pauseSession();
    expect(store.getState().sessionPaused).toBe(true);

    store.getState().revealNext();
    // Paused routes through the debug loop even with nothing armed…
    expect(session.debugStepLine).toHaveBeenCalledWith("over");
    // …and an ordinary step keeps the paused state (stepping IS how a
    // paused session moves).
    expect(store.getState().sessionPaused).toBe(true);
  });

  it("an explicit transport step leaves the session paused", () => {
    const session = scriptedSession();
    const { store } = bind(session);

    store.getState().debugStepLine("into");
    expect(session.debugStepLine).toHaveBeenCalledWith("into", undefined);
    expect(store.getState().sessionPaused).toBe(true);
  });

  it("noLineInfo falls back to the journaled single-line advance", () => {
    const session = scriptedSession([BP]);
    session.debugStepLine.mockReturnValue(outcome({ type: "noLineInfo" }));
    const { store } = bind(session);

    store.getState().revealNext();
    expect(session.continueSingle).toHaveBeenCalled();
    expect(store.getState().sessionText).toContain("prod line");
    expect(store.getState().sessionPaused).toBe(false);
  });

  it("a choices stop presents the runtime's pending choices", () => {
    const session = scriptedSession([BP]);
    session.debugStepLine.mockReturnValue(outcome({ type: "choices" }));
    session.debugSnapshot.mockReturnValue({
      status: "waiting_for_choice",
      pending_choices: [{ index: 0, text: "Order another" }],
    } as never);
    const { store } = bind(session);

    store.getState().revealNext();
    expect(store.getState().sessionStatus).toBe("awaiting-choice");
    expect(store.getState().sessionChoices).toEqual([
      { index: 0, text: "Order another", tags: [] },
    ]);
  });

  it("session creation re-arms the anchors on the fresh wasm session", () => {
    // Found live (W5 review): Run swaps the provider's internal session,
    // the runtime breakpoint set dies with the old one, and a solid
    // gutter dot sat over an EMPTY set — a breakpoint that never hits.
    // startSession/openSession must re-sync after provider.start().
    const store = createStudioStore();
    const sync = vi.fn();
    store.setState({ _syncSourceBreakpoints: sync });

    store.getState().startSession(new Uint8Array([1, 2, 3]));
    expect(sync).toHaveBeenCalled();

    sync.mockClear();
    store.setState({ storyBytes: new Uint8Array([1, 2, 3]) });
    store.getState().openSession({ label: "secondary" });
    expect(sync).toHaveBeenCalled();
  });

  it("restart abandons the pause point", () => {
    const session = scriptedSession();
    const { store } = bind(session);
    store.getState().pauseSession();
    expect(store.getState().sessionPaused).toBe(true);

    store.getState().restartSession();
    expect(store.getState().sessionPaused).toBe(false);
  });
});
