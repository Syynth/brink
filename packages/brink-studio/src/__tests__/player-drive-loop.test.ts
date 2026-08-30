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
    debugRunToLine: vi.fn((): DebugRunOutcome => outcome({ type: "step" })),
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

  it("with breakpoints armed, a single reveal runs to the next content line", () => {
    const session = scriptedSession([BP]);
    session.debugRunToLine.mockReturnValue(
      outcome({ type: "step" }, [{ text: "next content line\n", tags: [] }]),
    );
    const { store } = bind(session);

    store.getState().revealNext();
    // 2026-08-30 Continue ruling: the reveal is the author-tier
    // content-line run, not a statement step — an author never grinds
    // through `~` statements to reach content.
    expect(session.debugRunToLine).toHaveBeenCalled();
    expect(session.debugStepLine).not.toHaveBeenCalled();
    expect(session.continueSingle).not.toHaveBeenCalled();
    // The outcome's lines delta reaches the transcript — the coherence
    // half: the crossed line must not be eaten.
    expect(store.getState().sessionText).toContain("next content line");
    // An ordinary playing reveal does not enter the paused state.
    expect(store.getState().sessionPaused).toBe(false);
  });

  it("with breakpoints armed and auto on, reveal free-runs to the next stop", () => {
    // The W7 paced default (F13) would pump line-by-line — this pin is
    // about the "all at once" batch road, so switch pacing off.
    const session = scriptedSession([BP]);
    const { store, provider } = bind(session);
    provider.setAuto(true);
    store.getState().setSessionPaced(0);

    store.getState().revealNext();
    expect(session.debugRun).toHaveBeenCalled();
  });

  it("a breakpoint hit pauses the session; Continue (debugRunToLine) resumes", () => {
    const session = scriptedSession([BP]);
    session.debugRunToLine.mockReturnValue(
      outcome({ type: "breakpoint", id: 1, name: "bp" }),
    );
    const { store } = bind(session);

    store.getState().revealNext();
    expect(store.getState().sessionPaused).toBe(true);

    // Continue: deliver the next content line and resume play (2026-08-30
    // ruling) — paused clears on an ordinary step stop.
    session.debugRunToLine.mockReturnValue(
      outcome({ type: "step" }, [{ text: "after the stop\n", tags: [] }]),
    );
    store.getState().debugRunToLine();
    expect(store.getState().sessionPaused).toBe(false);
    expect(store.getState().sessionText).toContain("after the stop");
  });

  it("the pause verb pauses; a paused reveal delivers the next content line and RESUMES", () => {
    // 2026-08-30 Continue ruling — this REVERSES the W5 pin ("a paused
    // reveal line-steps and stays paused"): the reveal-while-paused click
    // IS Continue. Staying paused is what the statement steps are for.
    const session = scriptedSession(); // no breakpoints at all
    const { store } = bind(session);

    store.getState().pauseSession();
    expect(store.getState().sessionPaused).toBe(true);

    store.getState().revealNext();
    // Paused routes through the debug loop even with nothing armed…
    expect(session.debugRunToLine).toHaveBeenCalled();
    // …and the ordinary content-line stop RESUMES play.
    expect(store.getState().sessionPaused).toBe(false);
  });

  it("choosing while paused delivers the consequence and STAYS paused (F7)", () => {
    // The 2026-08-30 resume ruling covers the Continue/reveal gesture
    // only — F7's paused choice presentation is unchanged: pick while
    // paused, inspect the consequence, still paused.
    const session = scriptedSession();
    session.debugSnapshot.mockReturnValue({
      status: "waiting_for_choice",
      pending_choices: [{ index: 0, text: "Pick me" }],
    } as never);
    const { store, provider } = bind(session);
    provider.pause();

    session.debugRunToLine.mockReturnValue(
      outcome({ type: "step" }, [{ text: "the consequence\n", tags: [] }]),
    );
    store.getState().chooseOption(0);
    expect(store.getState().sessionPaused).toBe(true);
    expect(store.getState().sessionText).toContain("the consequence");
  });

  it("an explicit transport step leaves the session paused", () => {
    const session = scriptedSession();
    const { store } = bind(session);

    store.getState().debugStepLine("into");
    expect(session.debugStepLine).toHaveBeenCalledWith("into", undefined);
    expect(store.getState().sessionPaused).toBe(true);
  });

  it("the reveal road needs no debug line info (stripped artifacts still play)", () => {
    // The content-line run's stop condition is output-buffer state, not a
    // DebugInfo entry — the W5-era noLineInfo fallback to the journaled
    // road is gone because the verb itself never degrades. A stripped
    // artifact reveals through the same debug road, breakpoint-bounded.
    const session = scriptedSession([BP]);
    session.hasDebugInfo.mockReturnValue(false);
    session.debugRunToLine.mockReturnValue(
      outcome({ type: "step" }, [{ text: "still plays\n", tags: [] }]),
    );
    const { store } = bind(session);

    store.getState().revealNext();
    expect(session.debugRunToLine).toHaveBeenCalled();
    expect(store.getState().sessionText).toContain("still plays");
    expect(store.getState().sessionPaused).toBe(false);
  });

  it("a choices stop presents the runtime's pending choices", () => {
    const session = scriptedSession([BP]);
    session.debugRunToLine.mockReturnValue(outcome({ type: "choices" }));
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
