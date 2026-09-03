/**
 * `subscribeDebugRefresh` (W4/#3297 + W6/#3299) — the store subscription
 * that drives the editors' debug adornments, extracted from `mount.tsx`
 * for exactly one reason: its re-entrancy discipline is load-bearing.
 *
 * `_syncSourceBreakpoints` ends in a synchronous `set(...)` with fresh
 * array identities, and zustand notifies subscribers synchronously — so
 * the listener re-enters ITSELF mid-flight. The original inline version
 * derived its change flags from a `last` snapshot it only updated after
 * the side effects: the re-entrant call saw the stale snapshot,
 * re-derived "program changed", re-synced, and recursed until the stack
 * blew (found live — the provider's load-error catch swallowed it, so
 * the only symptom was "Load error: Maximum call stack size exceeded"
 * in the Player and every debug surface silently dark).
 */
import { describe, expect, it, vi } from "vitest";
import { createStudioStore } from "@brink/studio-store";
import {
  subscribeDebugRefresh,
  type DebugRefreshTargets,
} from "../debug-refresh-subscription";

function targets() {
  return {
    refreshBreakpoints: vi.fn<() => void>(),
    refreshExecutionHighlight: vi.fn<() => void>(),
    revealProgram: vi.fn<(containerIdx: number, offset: number) => void>(),
  } satisfies DebugRefreshTargets;
}

describe("subscribeDebugRefresh — follow the Player (#3437)", () => {
  const src = { file: "main.ink", range_start: 10, range_end: 20 };
  const line = (text: string, withSource = true) =>
    ({ text, kind: "line" as const, tags: [], ...(withSource ? { source: src } : {}) });

  it("a newly revealed line scrolls the editor to its source while playing with follow on", () => {
    const store = createStudioStore();
    store.setState({ sessionStatus: "running", followInEditor: true } as never);
    const t = { ...targets(), followSource: vi.fn<(s: typeof src) => void>() };
    subscribeDebugRefresh(store, t);
    store.setState({ sessionLines: [line("One")] } as never);
    expect(t.followSource).toHaveBeenCalledWith(src);
    expect(t.refreshExecutionHighlight).toHaveBeenCalled();
  });

  it("follows the newest line that HAS a source, not a source-less notice", () => {
    const store = createStudioStore();
    store.setState({ sessionStatus: "running", followInEditor: true } as never);
    const t = { ...targets(), followSource: vi.fn<(s: typeof src) => void>() };
    subscribeDebugRefresh(store, t);
    const other = { file: "main.ink", range_start: 30, range_end: 40 };
    store.setState({
      sessionLines: [line("One"), { ...line("Two"), source: other }, line("notice", false)],
    } as never);
    expect(t.followSource).toHaveBeenLastCalledWith(other);
  });

  it("does not follow when off, when paused by an edit, at a debugger pause, or when idle", () => {
    for (const state of [
      { sessionStatus: "running", followInEditor: false },
      { sessionStatus: "running", followInEditor: true, followPaused: true },
      { sessionStatus: "running", followInEditor: true, sessionPaused: true },
      { sessionStatus: "none", followInEditor: true },
    ]) {
      const store = createStudioStore();
      store.setState(state as never);
      const t = { ...targets(), followSource: vi.fn<(s: typeof src) => void>() };
      subscribeDebugRefresh(store, t);
      store.setState({ sessionLines: [line("One")] } as never);
      expect(t.followSource, JSON.stringify(state)).not.toHaveBeenCalled();
    }
  });

  it("a hover change and a follow flip refresh the highlight; a new run lifts an edit's pause", () => {
    const store = createStudioStore();
    store.setState({ sessionStatus: "none", followInEditor: true, followPaused: true } as never);
    const t = targets();
    subscribeDebugRefresh(store, t);
    store.getState().setSessionHoverSource(src);
    expect(t.refreshExecutionHighlight).toHaveBeenCalledTimes(1);
    store.getState().setFollowInEditor(false);
    expect(t.refreshExecutionHighlight).toHaveBeenCalledTimes(2);
    store.setState({ followInEditor: true, followPaused: true } as never);
    store.setState({ sessionStatus: "running" } as never);
    expect(store.getState().followPaused).toBe(false);
  });
});

describe("subscribeDebugRefresh (W4/#3297, W6/#3299)", () => {
  it("terminates when the sync itself setStates synchronously (the live stack overflow)", () => {
    const store = createStudioStore();
    let syncs = 0;
    store.setState({
      _syncSourceBreakpoints: () => {
        syncs += 1;
        // Guard the test itself against the regression it pins.
        expect(syncs).toBeLessThan(10);
        // The real sync always ends in a set() with fresh identities —
        // reproduce that synchronous re-notification here.
        store.setState({ sourceBreakpoints: [] });
      },
    });
    const t = targets();
    subscribeDebugRefresh(store, t);

    store.setState({ programChecksum: "0xnew" });

    // Exactly one re-arm for one program change — the re-entrant
    // notification sees the already-updated snapshot and derives no
    // programChanged of its own.
    expect(syncs).toBe(1);
    expect(t.refreshBreakpoints).toHaveBeenCalled();
  });

  it("a program-identity change re-arms; an anchor-only change just re-renders", () => {
    const store = createStudioStore();
    const sync = vi.fn();
    store.setState({ _syncSourceBreakpoints: sync });
    const t = targets();
    subscribeDebugRefresh(store, t);

    store.setState({ sourceBreakpoints: [] });
    expect(sync).not.toHaveBeenCalled();
    expect(t.refreshBreakpoints).toHaveBeenCalledTimes(1);

    store.setState({ programChecksum: "0xswap" });
    expect(sync).toHaveBeenCalledTimes(1);
    expect(t.refreshBreakpoints).toHaveBeenCalledTimes(2);
  });

  it("the highlight refreshes when the runtime moves, not on unrelated state", () => {
    const store = createStudioStore();
    const t = targets();
    subscribeDebugRefresh(store, t);

    store.setState({ sessionText: ["a line"] });
    expect(t.refreshExecutionHighlight).not.toHaveBeenCalled();

    store.setState({
      debugState: { position: { container_idx: 1, offset: 2 } } as never,
    });
    expect(t.refreshExecutionHighlight).toHaveBeenCalledTimes(1);

    store.setState({ sessionPaused: true });
    expect(t.refreshExecutionHighlight).toHaveBeenCalledTimes(2);
  });

  it("reveal-on-stop fires once on the paused RISING edge, at the stop position", () => {
    const store = createStudioStore();
    const t = targets();
    subscribeDebugRefresh(store, t);

    store.setState({
      debugState: { position: { container_idx: 7, offset: 42 } } as never,
    });
    expect(t.revealProgram).not.toHaveBeenCalled();

    store.setState({ sessionPaused: true });
    expect(t.revealProgram).toHaveBeenCalledTimes(1);
    expect(t.revealProgram).toHaveBeenCalledWith(7, 42);

    // Still paused, other state moves: the edge is consumed.
    store.setState({ sessionText: ["more"] });
    expect(t.revealProgram).toHaveBeenCalledTimes(1);

    // Resume then pause again: a new stop, a new reveal.
    store.setState({ sessionPaused: false });
    store.setState({ sessionPaused: true });
    expect(t.revealProgram).toHaveBeenCalledTimes(2);
  });

  it("unsubscribe detaches cleanly", () => {
    const store = createStudioStore();
    const t = targets();
    const off = subscribeDebugRefresh(store, t);
    off();
    store.setState({ sessionPaused: true, debugState: { position: { container_idx: 0, offset: 0 } } as never });
    expect(t.revealProgram).not.toHaveBeenCalled();
    expect(t.refreshExecutionHighlight).not.toHaveBeenCalled();
  });
});
