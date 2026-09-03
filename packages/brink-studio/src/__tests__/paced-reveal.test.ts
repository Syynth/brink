/**
 * W7/#3300 F13 — the paced auto-reveal (RULED: paced by default, one
 * line at a time in rapid succession; pause/breakpoint stops the run
 * INSTANTLY — nothing is queued because each tick advances the VM one
 * line, so stopping the pump is the whole flush; stepping is never
 * paced).
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createStudioStore, LocalSessionProvider } from "@brink/studio-store";
import type { Breakpoint, DebugRunOutcome } from "@brink/wasm-types";

type Line = { type: string; text: string; tags: string[] };

function outcome(
  reason: DebugRunOutcome["reason"],
  lines: { text: string; tags: string[] }[] = [],
): DebugRunOutcome {
  return { reason, depth: 1, lines };
}

function scriptedSession(feed: Line[]) {
  let i = 0;
  return {
    continueSingle: vi.fn((): Line => {
      const line = feed[Math.min(i, feed.length - 1)];
      i += 1;
      return line;
    }),
    continueToPause: vi.fn((): Line[] => feed.slice(i)),
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
    debugBreakpoints: vi.fn((): Breakpoint[] => []),
    debugRun: vi.fn((): DebugRunOutcome => outcome({ type: "terminal" })),
    debugStep: vi.fn((): DebugRunOutcome => outcome({ type: "step" })),
    debugStepLine: vi.fn((): DebugRunOutcome => outcome({ type: "step" })),
    debugRunToLine: vi.fn((): DebugRunOutcome => outcome({ type: "step" })),
  };
}

function line(text: string): Line {
  return { type: "text", text: `${text}\n`, tags: [] };
}

function bind(feed: Line[]) {
  const store = createStudioStore();
  const provider = new LocalSessionProvider({
    session: scriptedSession(feed) as never,
    status: "running",
  });
  store.getState()._bindProvider(provider);
  return { store, provider };
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("paced auto-reveal (W7/#3300 F13)", () => {
  it("with auto on and a cadence set, one reveal pumps the run line by line", () => {
    const { store, provider } = bind([
      line("one"),
      line("two"),
      line("three"),
      { type: "choices", text: "", tags: [] },
    ]);
    provider.setAuto(true);
    store.getState().setSessionPaced(150);

    store.getState().revealNext();
    expect(store.getState().sessionText).toEqual(["one"]);
    expect(provider.pacedRunning()).toBe(true);

    vi.advanceTimersByTime(150);
    expect(store.getState().sessionText).toEqual(["one", "two"]);

    vi.advanceTimersByTime(150);
    expect(store.getState().sessionText).toEqual(["one", "two", "three"]);

    // The choices stop ends the run — the pump dies with it.
    vi.advanceTimersByTime(150);
    expect(store.getState().sessionStatus).toBe("awaiting-choice");
    vi.advanceTimersByTime(1000);
    expect(provider.pacedRunning()).toBe(false);
    expect(store.getState().sessionText).toEqual(["one", "two", "three"]);
  });

  it("pause stops the pump instantly (the ruled flush)", () => {
    const { store, provider } = bind([
      line("one"),
      line("two"),
      line("three"),
      line("four"),
    ]);
    provider.setAuto(true);
    store.getState().setSessionPaced(150);

    store.getState().revealNext();
    vi.advanceTimersByTime(150);
    expect(store.getState().sessionText).toEqual(["one", "two"]);

    store.getState().pauseSession();
    expect(provider.pacedRunning()).toBe(false);
    vi.advanceTimersByTime(1000);
    expect(store.getState().sessionText).toEqual(["one", "two"]);
    expect(store.getState().sessionPaused).toBe(true);
  });

  it("turning auto off mid-run abandons the pump", () => {
    const { store, provider } = bind([line("one"), line("two"), line("three")]);
    provider.setAuto(true);
    store.getState().setSessionPaced(150);
    store.getState().revealNext();
    expect(provider.pacedRunning()).toBe(true);

    provider.setAuto(false);
    expect(provider.pacedRunning()).toBe(false);
    vi.advanceTimersByTime(1000);
    expect(store.getState().sessionText).toEqual(["one"]);
  });

  it("cadence 0 is 'all at once' — the batch road, no pump", () => {
    const { store, provider } = bind([
      line("one"),
      line("two"),
      { type: "choices", text: "", tags: [] },
    ]);
    provider.setAuto(true);
    store.getState().setSessionPaced(0);

    store.getState().revealNext();
    expect(store.getState().sessionText).toEqual(["one", "two"]);
    expect(provider.pacedRunning()).toBe(false);
  });

  it("the cadence set BEFORE a provider binds is applied at bind", () => {
    const store = createStudioStore();
    store.getState().setSessionPaced(80);
    const provider = new LocalSessionProvider({
      session: scriptedSession([line("one"), line("two")]) as never,
      status: "running",
    });
    provider.setAuto(true);
    store.getState()._bindProvider(provider);

    store.getState().revealNext();
    expect(provider.pacedRunning()).toBe(true);
  });

  // ── Fast-forward = one-shot ContinueMaximally (RULED 2026-08-30) ────
  it("continueMaximally runs the batch road once, with no sticky auto", () => {
    const { store, provider } = bind([
      line("one"),
      line("two"),
      { type: "end", text: "", tags: [] },
    ]);
    store.getState().setSessionPaced(0); // all-at-once App setting
    store.getState().revealMaximally();

    const session = (provider as unknown as { session: ReturnType<typeof scriptedSession> })
      .session;
    // One line per wasm call (ruled 2026-09-02, "TS steps single lines"):
    // the batch road steps continueSingle until the stop, never the
    // wasm-side batch.
    expect(session.continueToPause).not.toHaveBeenCalled();
    expect(session.continueSingle).toHaveBeenCalledTimes(3);
    expect(store.getState().sessionText.join(" ")).toContain("one");
    expect(store.getState().sessionText.join(" ")).toContain("two");
    // One shot — the mode toggle it replaced must NOT flip.
    expect(store.getState().sessionAuto).toBe(false);
    // …and the next ordinary reveal is single-line again.
    store.getState().revealNext();
    expect(session.continueToPause).not.toHaveBeenCalled();
  });

  it("continueMaximally honors the paced setting — pump to the stop, auto untouched", () => {
    const { store, provider } = bind([
      line("one"),
      line("two"),
      { type: "end", text: "", tags: [] },
    ]);
    store.getState().setSessionPaced(150);
    store.getState().revealMaximally();

    // First line lands now; the pump carries the rest on the cadence.
    expect(store.getState().sessionText.join(" ")).toContain("one");
    expect(provider.pacedRunning()).toBe(true);
    vi.advanceTimersByTime(150);
    expect(store.getState().sessionText.join(" ")).toContain("two");
    vi.advanceTimersByTime(600);
    expect(store.getState().sessionStatus).toBe("ended");
    expect(provider.pacedRunning()).toBe(false);
    expect(store.getState().sessionAuto).toBe(false);
  });

  it("a breakpoint hit mid-run pauses and stops the pump", () => {
    const { store, provider } = bind([line("one"), line("two")]);
    provider.setAuto(true);
    store.getState().setSessionPaced(150);
    store.getState().revealNext();

    // Arm a breakpoint mid-run: the next tick routes through the debug
    // road and reports a hit.
    const session = (provider as unknown as { session: ReturnType<typeof scriptedSession> })
      .session;
    session.debugBreakpoints.mockReturnValue([
      { id: 1, container_idx: 0, offset: 4, name: "bp", enabled: true },
    ]);
    session.debugRunToLine.mockReturnValue(outcome({ type: "breakpoint", id: 1, name: "bp" }));

    vi.advanceTimersByTime(150);
    expect(store.getState().sessionPaused).toBe(true);
    expect(provider.pacedRunning()).toBe(false);
    vi.advanceTimersByTime(1000);
    expect(session.debugRunToLine).toHaveBeenCalledTimes(1);
  });
});
