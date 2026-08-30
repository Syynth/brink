/**
 * Debug session slice + commands (D8's control bridge, #3232).
 *
 * Covers: `debugCapable` mirroring the bound provider's "debug" capability,
 * the `debug.*` commands' `when` gating, breakpoint add/remove/toggle
 * round-tripping through the provider, and `debugStatus` derivation from
 * `debugRun`/`debugStep` outcomes. Mirrors `story-session.test.ts`'s
 * scripted-session pattern — a plain fake object standing in for the real
 * `StorySessionHandle` (`@brink-lang/web`), whose actual production
 * behavior is proven separately over a real `WebSession`
 * (`crates/brink-web/src/session.rs`'s `debug_control_tests`).
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  createStudioStore,
  LocalSessionProvider,
  type StudioStore,
} from "@brink/studio-store";
import { CommandRegistry } from "@brink/studio-shell";
import type { Breakpoint, DebugRunOutcome } from "@brink/wasm-types";
import { registerDebugCommands } from "../debug-commands.js";

type Line = { type: string; text: string; tags: string[] };

function scriptedSession() {
  return {
    continueSingle: vi.fn((): Line => ({ type: "done", text: "", tags: [] })),
    continueToPause: vi.fn((): Line[] => [{ type: "done", text: "", tags: [] }]),
    choose: vi.fn(),
    restart: vi.fn(),
    free: vi.fn(),
    goToPath: vi.fn(),
    setDevVisibilityOverride: vi.fn(),
    debugSnapshot: vi.fn(() => null),
    onJournalDirty: vi.fn(() => () => {}),
    resolveDebugPosition: vi.fn(() => null),
    debugBreakpointAdd: vi.fn((): number => 0),
    debugBreakpointRemove: vi.fn((): boolean => true),
    debugBreakpointSetEnabled: vi.fn((): boolean => true),
    debugBreakpoints: vi.fn((): Breakpoint[] => []),
    debugRun: vi.fn((): DebugRunOutcome => ({ reason: { type: "terminal" }, depth: 0 })),
    debugStep: vi.fn((): DebugRunOutcome => ({ reason: { type: "step" }, depth: 1 })),
  };
}

function bindSession(
  store: StudioStore,
  session: Record<string, unknown>,
): LocalSessionProvider {
  const provider = new LocalSessionProvider({ session: session as never, status: "running" });
  store.getState()._bindProvider(provider);
  return provider;
}

beforeEach(() => {
  localStorage.clear();
});

describe("debugCapable", () => {
  it("is false before any session binds", () => {
    const store = createStudioStore();
    expect(store.getState().debugCapable).toBe(false);
    expect(store.getState().debugBreakpoints).toEqual([]);
  });

  it("becomes true once a local session binds, and mirrors its breakpoints", () => {
    const store = createStudioStore();
    const session = scriptedSession();
    session.debugBreakpoints.mockReturnValue([
      { id: 0, container_idx: 0, offset: 0, name: "entry", enabled: true },
    ]);
    bindSession(store, session);

    expect(store.getState().debugCapable).toBe(true);
    expect(store.getState().debugBreakpoints).toHaveLength(1);
  });

  it("goes false again on stopSession/disposeSession", () => {
    const store = createStudioStore();
    bindSession(store, scriptedSession());
    expect(store.getState().debugCapable).toBe(true);

    store.getState().disposeSession();
    expect(store.getState().debugCapable).toBe(false);
    expect(store.getState().debugBreakpoints).toEqual([]);
  });
});

describe("debug.* commands gating", () => {
  it("is disabled with no session bound", () => {
    const store = createStudioStore();
    const commands = new CommandRegistry();
    registerDebugCommands(commands, store);

    for (const id of [
      "debug.run",
      "debug.stepInto",
      "debug.stepOver",
      "debug.stepOut",
      "debug.breakpointAdd",
      "debug.breakpointRemove",
      "debug.breakpointToggle",
    ]) {
      expect(commands.isEnabled(id)).toBe(false);
    }
  });

  it("is enabled once a debug-capable session binds", () => {
    const store = createStudioStore();
    const commands = new CommandRegistry();
    registerDebugCommands(commands, store);
    bindSession(store, scriptedSession());

    expect(commands.isEnabled("debug.run")).toBe(true);
    expect(commands.isEnabled("debug.stepInto")).toBe(true);
  });
});

describe("breakpoint add/remove/toggle", () => {
  it("round-trips through the provider and refreshes debugBreakpoints", () => {
    const store = createStudioStore();
    const session = scriptedSession();
    session.debugBreakpointAdd.mockReturnValue(7);
    session.debugBreakpoints.mockReturnValue([
      { id: 7, container_idx: 1, offset: 2, name: "x", enabled: true },
    ]);
    bindSession(store, session);

    const id = store.getState().debugBreakpointAdd(1, 2, "x");
    expect(id).toBe(7);
    expect(session.debugBreakpointAdd).toHaveBeenCalledWith(1, 2, "x");
    expect(store.getState().debugBreakpoints).toEqual([
      { id: 7, container_idx: 1, offset: 2, name: "x", enabled: true },
    ]);

    store.getState().debugBreakpointToggle(7, false);
    expect(session.debugBreakpointSetEnabled).toHaveBeenCalledWith(7, false);

    store.getState().debugBreakpointRemove(7);
    expect(session.debugBreakpointRemove).toHaveBeenCalledWith(7);
  });

  it("no-ops without a bound session", () => {
    const store = createStudioStore();
    expect(store.getState().debugBreakpointAdd(0, 0)).toBe(-1);
    store.getState().debugBreakpointRemove(0); // must not throw
    store.getState().debugBreakpointToggle(0, true); // must not throw
  });
});

describe("debugRun / debugStep and debugStatus", () => {
  it("debugRun reaching a terminal outcome reports debugStatus 'stopped'", () => {
    const store = createStudioStore();
    const session = scriptedSession();
    session.debugRun.mockReturnValue({ reason: { type: "terminal" }, depth: 0 });
    bindSession(store, session);

    store.getState().debugRun();
    expect(session.debugRun).toHaveBeenCalledWith(undefined);
    expect(store.getState().debugLastOutcome).toEqual({
      reason: { type: "terminal" },
      depth: 0,
    });
    expect(store.getState().debugStatus).toBe("stopped");
  });

  it("debugRun reaching a breakpoint reports debugStatus 'paused'", () => {
    const store = createStudioStore();
    const session = scriptedSession();
    session.debugRun.mockReturnValue({
      reason: { type: "breakpoint", id: 3, name: "bp" },
      position: { container_idx: 0, offset: 5 },
      depth: 1,
    });
    bindSession(store, session);

    store.getState().debugRun(50_000);
    expect(session.debugRun).toHaveBeenCalledWith(50_000);
    expect(store.getState().debugStatus).toBe("paused");
  });

  it("debugStep drives the provider with the requested mode", () => {
    const store = createStudioStore();
    const session = scriptedSession();
    bindSession(store, session);

    store.getState().debugStep("into");
    expect(session.debugStep).toHaveBeenCalledWith("into", undefined);
    expect(store.getState().debugStatus).toBe("paused"); // scriptedSession's default: "step"
  });

  it("a choices outcome reports debugStatus 'stopped', matching terminal", () => {
    const store = createStudioStore();
    const session = scriptedSession();
    session.debugRun.mockReturnValue({ reason: { type: "choices" }, depth: 0 });
    bindSession(store, session);

    store.getState().debugRun();
    expect(store.getState().debugStatus).toBe("stopped");
  });

  it("no-ops without a bound session", () => {
    const store = createStudioStore();
    store.getState().debugRun(); // must not throw
    store.getState().debugStep("into"); // must not throw
    expect(store.getState().debugLastOutcome).toBeNull();
    expect(store.getState().debugStatus).toBe("none");
  });
});

// ── #3229: the per-session debug-info compile toggle ────────────────────
//
// The store half of the ruling. The Rust half — that the flag actually
// changes what `EditorSession::compile_project` emits, and that the studio's
// own bytes then resolve a position — is proven over the production road in
// `crates/brink-web/src/editor/mod.rs`. What matters *here* is that the
// store drives that session method and recompiles, because a toggle that
// sets the flag without recompiling looks like it works and changes nothing:
// the flag governs the NEXT compile, and the live session runs on the bytes
// the last one produced.

describe("setDebugInfoEnabled (#3229)", () => {
  function storeWithProject() {
    const store = createStudioStore();
    const setDebugInfoEnabled = vi.fn();
    const triggerCompile = vi.fn();
    store.setState({
      _project: {
        getSession: () => ({ setDebugInfoEnabled }),
      } as never,
      _documents: { triggerCompile } as never,
    });
    return { store, setDebugInfoEnabled, triggerCompile };
  }

  it("is ON by default — the W1/#3294 ruling, mirroring the wasm session's own default", () => {
    expect(createStudioStore().getState().debugInfoEnabled).toBe(true);
  });

  it("pushes the opt-out to the session AND recompiles", () => {
    const { store, setDebugInfoEnabled, triggerCompile } = storeWithProject();

    store.getState().setDebugInfoEnabled(false);

    expect(store.getState().debugInfoEnabled).toBe(false);
    expect(setDebugInfoEnabled).toHaveBeenCalledWith(false);
    // The recompile is the half that makes the toggle observable at all.
    expect(triggerCompile).toHaveBeenCalledTimes(1);
  });

  it("turns back on again — the opt-out is not a one-way door", () => {
    const { store, setDebugInfoEnabled, triggerCompile } = storeWithProject();

    store.getState().setDebugInfoEnabled(false);
    store.getState().setDebugInfoEnabled(true);

    expect(store.getState().debugInfoEnabled).toBe(true);
    expect(setDebugInfoEnabled).toHaveBeenLastCalledWith(true);
    expect(triggerCompile).toHaveBeenCalledTimes(2);
  });

  it("no-ops when unchanged, so a toggle can be driven without churning compiles", () => {
    const { store, setDebugInfoEnabled, triggerCompile } = storeWithProject();

    // Setting the default value is a pure no-op — nothing pushed, nothing
    // recompiled. This is what makes bootstrap's unconditional restore of
    // the persisted setting free for the (default) opted-in case.
    store.getState().setDebugInfoEnabled(true);
    expect(setDebugInfoEnabled).toHaveBeenCalledTimes(0);
    expect(triggerCompile).toHaveBeenCalledTimes(0);

    store.getState().setDebugInfoEnabled(false);
    store.getState().setDebugInfoEnabled(false);
    store.getState().setDebugInfoEnabled(false);

    expect(setDebugInfoEnabled).toHaveBeenCalledTimes(1);
    expect(triggerCompile).toHaveBeenCalledTimes(1);
  });

  it("records the opt-out even with no project bound, so a later bind is not silently on", () => {
    const store = createStudioStore();
    store.getState().setDebugInfoEnabled(false);
    expect(store.getState().debugInfoEnabled).toBe(false);
  });

  it("initialize applies a pre-seeded opt-out to the session before the first compile", () => {
    // The bootstrap order (W1/#3294): mount restores the persisted setting
    // pre-bind (state only — no project yet), then `initialize` pushes an
    // explicit opt-out to the session ahead of the first compile, so the
    // first bytes already honour it.
    const store = createStudioStore();
    store.getState().setDebugInfoEnabled(false);

    const setDebugInfoEnabled = vi.fn();
    const setExternalCheck = vi.fn();
    const triggerCompile = vi.fn();
    store.getState().initialize(
      { getSession: () => ({ setDebugInfoEnabled, setExternalCheck }) } as never,
      { triggerCompile } as never,
    );

    expect(setDebugInfoEnabled).toHaveBeenCalledWith(false);
    // The push must precede the compile it exists to influence.
    expect(setDebugInfoEnabled.mock.invocationCallOrder[0]).toBeLessThan(
      triggerCompile.mock.invocationCallOrder[0] ?? Infinity,
    );
  });

  it("initialize leaves the session's own default alone when not opted out", () => {
    const store = createStudioStore();
    const setDebugInfoEnabled = vi.fn();
    const setExternalCheck = vi.fn();
    const triggerCompile = vi.fn();
    store.getState().initialize(
      { getSession: () => ({ setDebugInfoEnabled, setExternalCheck }) } as never,
      { triggerCompile } as never,
    );
    // No call at all: the wasm session already defaults ON, and pushing
    // `true` would only churn the config generation for nothing.
    expect(setDebugInfoEnabled).not.toHaveBeenCalled();
    expect(triggerCompile).toHaveBeenCalledTimes(1);
  });
});
