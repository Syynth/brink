/**
 * Debug session slice — D8's breakpoint/pause/step bridged through wasm
 * (issue #3232), the studio-store side of `docs/debugger-spec.md`.
 *
 * Owns the studio's view of a debug-driven session: which breakpoints are
 * armed and the outcome of the most recent `debugRun`/`debugStep` call.
 * Mirrored from the active session's provider whenever it implements
 * `DebugSessionProvider` (`session/types.ts`) — today that's exactly the
 * local (wasm) provider; `debugCapable` is what lets a view (or a command's
 * `when`) find out without probing the provider itself.
 *
 * Like `SessionSlice`, this slice owns state — no view mutates the debug
 * session directly. Commands (`debug.run` / `debug.stepInto` / `debug.
 * stepOver` / `debug.stepOut` / `debug.breakpointAdd` / `debug.
 * breakpointRemove` / `debug.breakpointToggle`, registered at the app
 * boundary alongside `story.*`) are the implementation these slice actions
 * back.
 *
 * SCOPE (#3232's own "Scope Honesty"): this plumbing is real and proven
 * against a real `WebSession`/`StorySessionHandle` (see
 * `crates/brink-web/src/session.rs`'s `debug_control_tests` and this
 * package's own vitest suite) — but nothing in the studio can compile a
 * program WITH debug info yet (#3229 is the separate, un-made ruling on
 * the toggle mechanism). Until that lands, `debugBreakpoints`/`debugRun`/
 * `debugStep` are inert against an ordinary compile: a breakpoint position
 * has nothing to resolve to, and `debugRun` just runs to the story's own
 * terminal outcome having never matched one. No view consumes this slice
 * yet either — the editor gutter / current-line highlight is a separate,
 * later ticket (#3232's own "Not in scope").
 */

import type { StateCreator } from "zustand";
import type { Breakpoint, DebugRunOutcome, StepMode } from "@brink/wasm-types";
import type { StudioState } from "../index.js";

import { isDebugSessionProvider } from "../session/types.js";

// Re-exported for back-compat: consumers import these from the store root.
export { isDebugSessionProvider, type DebugSessionProvider } from "../session/types.js";

export interface DebugSlice {
  /** Whether the active session's provider supports debug control — mirrors
   * `capabilities.has("debug")` as its own reactive field so views/commands
   * don't re-derive a `Set` membership check every render. Refreshed on
   * every session bind/switch/dispose (`_refreshDebugCapability`). */
  debugCapable: boolean;
  /** Breakpoints armed on the active session, mirrored after every
   * add/remove/toggle and on session switch. Empty when the provider isn't
   * debug-capable or no session is active — never "stale" in that case,
   * there is nothing armed to report. */
  debugBreakpoints: Breakpoint[];
  /** The outcome of the most recent `debugRun`/`debugStep` call; `null`
   * before either has been called on the current session (including right
   * after a session switch/dispose — a debug pause point doesn't carry
   * across sessions). */
  debugLastOutcome: DebugRunOutcome | null;
  /**
   * Derived from `debugLastOutcome.reason.type`:
   * - `"none"` — no debug-driven stepping has happened yet this session.
   * - `"paused"` — stopped at a breakpoint/watchpoint/step boundary; another
   *   `debugRun`/`debugStep` can usefully continue from here.
   * - `"stopped"` — the last call reached a terminal VM outcome or a choice
   *   point; the ordinary `story.continue`/`story.choose` verbs are what
   *   move the session next, not more debug-stepping.
   */
  debugStatus: "none" | "paused" | "stopped";

  /** Add an enabled breakpoint at a bytecode position, refreshing
   * `debugBreakpoints`. Returns -1 (never a real id) without a debug-capable
   * live session. */
  debugBreakpointAdd(containerIdx: number, offset: number, name?: string): number;
  /** Remove a breakpoint by id, refreshing `debugBreakpoints`. No-op without
   * a debug-capable live session. */
  debugBreakpointRemove(id: number): void;
  /** Enable/disable a breakpoint without removing it, refreshing
   * `debugBreakpoints`. No-op without a debug-capable live session. */
  debugBreakpointToggle(id: number, enabled: boolean): void;
  /** Run to the next breakpoint/choice/terminal outcome. Updates
   * `debugLastOutcome`/`debugStatus` and refreshes the State View (the run
   * lands the flow at a new position). No-op without a debug-capable live
   * session. */
  debugRun(budgetCeiling?: number): void;
  /** Step by one `StepMode` unit ("into" | "over" | "out"). Same
   * update/refresh contract as `debugRun`. */
  debugStep(mode: StepMode, budgetCeiling?: number): void;
  /**
   * Whether this editor session compiles with the D6 `DebugInfo` section
   * (#3229). **Off by default**, and the reason matters: without it, every
   * source-position feature the debugger has — the current-line highlight,
   * breakpoint→source mapping, the locals panel — resolves to nothing,
   * however correct its code is.
   */
  debugInfoEnabled: boolean;
  /**
   * Turn the debug-info compile on or off for this session and recompile
   * (#3229, ruled 2026-08-28: per-session, not always-on and not a
   * `brink.toml` key). Turn it on when entering a debugging context and off
   * when leaving, so ordinary authoring never pays the extra compile size
   * and time.
   *
   * The recompile is not optional bookkeeping — the flag governs what the
   * NEXT compile emits, and the studio's live session runs on those bytes.
   * It is cheap: only codegen re-runs, diagnostics stay memoized.
   *
   * No-ops when the value is unchanged, so callers may drive it from a
   * toggle without churning compiles.
   */
  setDebugInfoEnabled(enabled: boolean): void;
  /** Refresh `debugCapable`/`debugBreakpoints` from the active provider —
   * called on session bind/switch/dispose (mirrors `_refreshDebugState`). */
  _refreshDebugCapability(): void;
}

function statusOfOutcome(outcome: DebugRunOutcome | null): DebugSlice["debugStatus"] {
  if (!outcome) return "none";
  switch (outcome.reason.type) {
    case "choices":
    case "terminal":
      return "stopped";
    default:
      return "paused";
  }
}

export const createDebugSlice: StateCreator<StudioState, [], [], DebugSlice> = (set, get) => ({
  debugCapable: false,
  debugBreakpoints: [],
  debugLastOutcome: null,
  debugStatus: "none",
  debugInfoEnabled: false,

  setDebugInfoEnabled(enabled) {
    if (get().debugInfoEnabled === enabled) return;
    set({ debugInfoEnabled: enabled });
    const project = get()._project;
    if (project !== null) {
      project.getSession().setDebugInfoEnabled(enabled);
      // Mirrors `setExternalCheck`: the session method changes what the
      // next compile produces, it does not produce it. Toggling bumps the
      // session generation, so this recompile is a real one rather than a
      // cache hit.
      get()._documents?.triggerCompile();
    }
  },

  debugBreakpointAdd(containerIdx, offset, name) {
    const provider = get()._provider;
    if (!provider || !isDebugSessionProvider(provider)) return -1;
    const id = provider.debugBreakpointAdd(containerIdx, offset, name);
    set({ debugBreakpoints: provider.debugBreakpoints() });
    return id;
  },

  debugBreakpointRemove(id) {
    const provider = get()._provider;
    if (!provider || !isDebugSessionProvider(provider)) return;
    provider.debugBreakpointRemove(id);
    set({ debugBreakpoints: provider.debugBreakpoints() });
  },

  debugBreakpointToggle(id, enabled) {
    const provider = get()._provider;
    if (!provider || !isDebugSessionProvider(provider)) return;
    provider.debugBreakpointSetEnabled(id, enabled);
    set({ debugBreakpoints: provider.debugBreakpoints() });
  },

  debugRun(budgetCeiling) {
    const provider = get()._provider;
    if (!provider || !isDebugSessionProvider(provider)) return;
    const outcome = provider.debugRun(budgetCeiling);
    set({ debugLastOutcome: outcome, debugStatus: statusOfOutcome(outcome) });
    // Pick up the position/call-stack/globals the run landed at.
    get()._refreshDebugState();
  },

  debugStep(mode, budgetCeiling) {
    const provider = get()._provider;
    if (!provider || !isDebugSessionProvider(provider)) return;
    const outcome = provider.debugStep(mode, budgetCeiling);
    set({ debugLastOutcome: outcome, debugStatus: statusOfOutcome(outcome) });
    get()._refreshDebugState();
  },

  _refreshDebugCapability() {
    const provider = get()._provider;
    if (provider && isDebugSessionProvider(provider)) {
      // `debugBreakpoints()` reads through to the live session, which can be
      // absent (a fresh/adopted provider that hasn't loaded yet) or, in a
      // test double, simply not implement the debug surface — never let a
      // routine refresh (run on every session bind/switch) throw into the
      // caller, matching `LocalSessionProvider.refreshDebug`'s own
      // try/catch discipline for the analogous `debugSnapshot()` read.
      let breakpoints: Breakpoint[] = [];
      try {
        breakpoints = provider.debugBreakpoints();
      } catch {
        breakpoints = [];
      }
      set({ debugCapable: true, debugBreakpoints: breakpoints });
    } else {
      set({
        debugCapable: false,
        debugBreakpoints: [],
        debugLastOutcome: null,
        debugStatus: "none",
      });
    }
  },
});
