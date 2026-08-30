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
 * SCOPE: the wasm plumbing is real and proven against a real
 * `WebSession`/`StorySessionHandle`; W1 (#3294) made every studio compile
 * carry debug info by default, so positions resolve for real. W4 (#3297)
 * adds the SOURCE-ANCHORED breakpoint model below: the author's
 * breakpoints are `(file, line)` anchors (range-keyed per D1's v1 ruling
 * — they re-anchor on recompile and may drift across edits), and the
 * runtime's `(container_idx, offset)` set is DERIVED state, re-bound via
 * the provider's `resolveSourceLine` on every sync. The editor gutter
 * consumes `sourceBreakpoints`; the raw `debugBreakpoints` mirror stays
 * what the runtime actually has armed.
 */

import type { StateCreator } from "zustand";
import type { Breakpoint, DebugRunOutcome, ProgramAddress, StepMode } from "@brink/wasm-types";
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

  /**
   * The author's breakpoints (W4/#3297): source-anchored `(file, line)`
   * entries — the STORED identity, per D1's range-keyed v1 ruling. The
   * runtime's `(container_idx, offset)` set is derived from these on every
   * sync; `address` records the latest binding (`null` = unbound: no
   * executable code on that line, no debug info, or no live session to
   * bind against — the gutter renders it hollow). `line` is 0-based.
   */
  sourceBreakpoints: SourceBreakpoint[];
  /** Toggle a breakpoint anchor at `(file, line)` — the gutter-click verb.
   * Adds an enabled anchor when none exists there, removes the existing
   * one otherwise. Syncs + persists. */
  breakpointToggleAtLine(file: string, line: number): void;
  /** Enable/disable an anchor without removing it (panel checkbox). */
  breakpointSetEnabled(key: string, enabled: boolean): void;
  /** Remove an anchor (panel ×). */
  breakpointRemove(key: string): void;
  /** Disable every anchor without removing any (panel header action). */
  breakpointsDisableAll(): void;
  /** Remove every anchor (panel header action). */
  breakpointsClearAll(): void;
  /**
   * The Debugger panel's selected stack frame (W8/#3301) — an index into
   * `debugState.call_stack`, or `null` for "the top frame" (the default).
   * Selection scopes the Variables section's locals, drives the editor's
   * accent frame band (W6's `"frame"` highlight kind), and reveals the
   * frame's position. Reset to `null` whenever the runtime advances — a
   * selection belongs to the stack it was made in.
   */
  selectedFrameIdx: number | null;
  /** Select a frame (Debugger panel click); `null` returns to the top. */
  selectFrame(idx: number | null): void;
  /**
   * "Reveal in Program Explorer" (W9/#3302): the instruction the explorer
   * should scroll to and flash — set by `revealInstructionsAt`, consumed
   * reactively by `ProgramView`. `nonce` distinguishes repeat reveals of
   * the same address.
   */
  programExplorerTarget: { address: ProgramAddress; nonce: number } | null;
  /**
   * Resolve a source line (0-based) to its instructions and target the
   * Program Explorer at them. Needs a live debug-capable session (the
   * source→address road is the session's resolver); without one — or on
   * a line with no statement — raises an honest notification instead.
   * Returns whether a target was set (the caller opens the tool window).
   */
  revealInstructionsAt(file: string, line: number): boolean;
  /** Apply editor change-mapping: the anchors in `file` whose lines moved
   * under an edit. `moves` pairs old→new 0-based lines; anchors whose line
   * isn't listed stay put. Two anchors mapped onto the same line collapse
   * into one (the edit deleted the text between them). */
  breakpointsMoved(file: string, moves: readonly { from: number; to: number }[]): void;
  /** Seed anchors from persistence at bootstrap (before any session), then
   * sync. Replaces the current set. */
  applyPersistedBreakpoints(list: readonly { file: string; line: number; enabled: boolean }[]): void;
  /** Where anchor changes are persisted to (per-project, wired at mount —
   * the `setProblemsPrefsSink` pattern). */
  setBreakpointsSink(
    sink: ((list: { file: string; line: number; enabled: boolean }[]) => void) | null,
  ): void;
  /** The wired sink; internal. */
  _breakpointsSink: ((list: { file: string; line: number; enabled: boolean }[]) => void) | null;
  /**
   * Re-derive every anchor's binding and re-arm the live session's runtime
   * breakpoint set from scratch. Called on session bind/switch (via
   * `_refreshDebugCapability`), on every compile result, and after any
   * anchor mutation. Without a debug-capable provider this only clears the
   * runtime mirror; anchor `address`es go `null` (nothing to bind against)
   * — the anchors themselves always survive.
   */
  _syncSourceBreakpoints(): void;

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
  /** Step by one SOURCE LINE (W5/#3298) — the transport's step verbs. */
  debugStepLine(mode: StepMode, budgetCeiling?: number): void;
  /** Continue (2026-08-30 ruling): run until the next CONTENT line is
   * delivered — or a breakpoint/choices/terminal stop — and resume play.
   * Same update/refresh contract as `debugRun`. */
  debugRunToLine(budgetCeiling?: number): void;
  /**
   * Whether this editor session compiles with the D6 `DebugInfo` section
   * (#3229). **ON by default since 2026-08-29** (W1/#3294, "debug info on
   * by default" — supersedes the earlier default-off consequence), and the
   * reason matters: with it, every source-position feature the debugger
   * has — the current-line highlight, breakpoint→source mapping, the
   * locals panel — resolves from the studio's own bytes with no toggle
   * touched. `false` only via the App-settings opt-out.
   */
  debugInfoEnabled: boolean;
  /**
   * Turn the debug-info compile on or off for this session and recompile
   * (#3229's mechanism; per-session, never a `brink.toml` key). Since
   * W1/#3294 the default is ON — this action's main caller is the
   * App-settings opt-out (and bootstrap restoring a persisted opt-out).
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

/** One source-anchored breakpoint (W4/#3297). See `DebugSlice.sourceBreakpoints`. */
export interface SourceBreakpoint {
  /** Stable identity across line moves and rebinds — never derived from
   * the position (a `file:line` key would change under the very edits the
   * anchor is meant to survive). */
  key: string;
  file: string;
  /** 0-based, matching the wasm resolvers; 1-based display converts at
   * the UI edge. */
  line: number;
  enabled: boolean;
  /** The latest binding, or `null` = unbound (hollow in the gutter). */
  address: ProgramAddress | null;
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

/** Module-scope so keys stay unique across store instances in tests; only
 * per-store uniqueness is load-bearing. */
let nextBreakpointKey = 0;

/** How far past a clicked line the toggle scans for the nearest following
 * bindable line (spec F2's DAP-style snapping). Bounded so a click in the
 * trailing whitespace of a file terminates instead of walking forever. */
const BREAKPOINT_SNAP_SCAN_LINES = 50;

export const createDebugSlice: StateCreator<StudioState, [], [], DebugSlice> = (set, get) => {
  /** The line resolver, when a debug-capable session is live. */
  const resolver = (): ((file: string, line: number) => ProgramAddress | null) | null => {
    const provider = get()._provider;
    if (!provider || !isDebugSessionProvider(provider)) return null;
    // Older test doubles narrow via the capability flag alone — probe the
    // method rather than trusting the cast.
    if (typeof provider.resolveSourceLine !== "function") return null;
    return (file, line) => {
      try {
        return provider.resolveSourceLine(file, line);
      } catch {
        return null;
      }
    };
  };

  const persistBreakpoints = (): void => {
    get()._breakpointsSink?.(
      get().sourceBreakpoints.map(({ file, line, enabled }) => ({ file, line, enabled })),
    );
  };

  return {
  debugCapable: false,
  debugBreakpoints: [],
  debugLastOutcome: null,
  debugStatus: "none",
  sourceBreakpoints: [],
  _breakpointsSink: null,

  breakpointToggleAtLine(file, line) {
    const anchors = get().sourceBreakpoints;
    // Snap first (spec F2, DAP convention): the anchor lands on the line
    // the click actually binds to, so toggling the snapped line's dot off
    // works on the line where the dot renders. Without a live resolver
    // (idle Player) the anchor stays where clicked and binds on the next
    // sync.
    const resolve = resolver();
    let target = line;
    if (resolve !== null && resolve(file, line) === null) {
      for (let probe = line + 1; probe <= line + BREAKPOINT_SNAP_SCAN_LINES; probe++) {
        if (resolve(file, probe) !== null) {
          target = probe;
          break;
        }
      }
    }
    const existing = anchors.find((a) => a.file === file && a.line === target);
    if (existing !== undefined) {
      set({ sourceBreakpoints: anchors.filter((a) => a.key !== existing.key) });
    } else {
      nextBreakpointKey += 1;
      set({
        sourceBreakpoints: [
          ...anchors,
          { key: `bp-${nextBreakpointKey}`, file, line: target, enabled: true, address: null },
        ],
      });
    }
    get()._syncSourceBreakpoints();
    persistBreakpoints();
  },

  breakpointSetEnabled(key, enabled) {
    const anchors = get().sourceBreakpoints;
    const hit = anchors.find((a) => a.key === key);
    if (hit === undefined || hit.enabled === enabled) return;
    set({
      sourceBreakpoints: anchors.map((a) => (a.key === key ? { ...a, enabled } : a)),
    });
    get()._syncSourceBreakpoints();
    persistBreakpoints();
  },

  breakpointRemove(key) {
    const anchors = get().sourceBreakpoints;
    if (!anchors.some((a) => a.key === key)) return;
    set({ sourceBreakpoints: anchors.filter((a) => a.key !== key) });
    get()._syncSourceBreakpoints();
    persistBreakpoints();
  },

  breakpointsDisableAll() {
    for (const b of get().sourceBreakpoints) {
      if (b.enabled) get().breakpointSetEnabled(b.key, false);
    }
  },

  breakpointsClearAll() {
    for (const b of [...get().sourceBreakpoints]) {
      get().breakpointRemove(b.key);
    }
  },

  selectedFrameIdx: null,

  selectFrame(idx) {
    set({ selectedFrameIdx: idx });
  },

  programExplorerTarget: null,

  revealInstructionsAt(file, line) {
    const resolve = resolver();
    if (resolve === null) {
      get()._notify?.({
        severity: "info",
        source: "story",
        message: "Start the story to map source lines to instructions.",
      });
      return false;
    }
    const address = resolve(file, line);
    if (address === null) {
      get()._notify?.({
        severity: "info",
        source: "story",
        message: `No compiled instructions for ${file}:${line + 1}.`,
      });
      return false;
    }
    set((s) => ({
      programExplorerTarget: {
        address,
        nonce: (s.programExplorerTarget?.nonce ?? 0) + 1,
      },
    }));
    return true;
  },

  breakpointsMoved(file, moves) {
    if (moves.length === 0) return;
    const byFrom = new Map(moves.map((m) => [m.from, m.to]));
    const seen = new Set<string>();
    const next: SourceBreakpoint[] = [];
    for (const a of get().sourceBreakpoints) {
      const moved = a.file === file ? byFrom.get(a.line) : undefined;
      const line = moved ?? a.line;
      // An edit that deletes the text between two anchors can map both
      // onto one line — keep the first, drop the duplicate.
      const at = `${a.file} ${line}`;
      if (seen.has(at)) continue;
      seen.add(at);
      next.push(moved === undefined ? a : { ...a, line });
    }
    set({ sourceBreakpoints: next });
    get()._syncSourceBreakpoints();
    persistBreakpoints();
  },

  applyPersistedBreakpoints(list) {
    set({
      sourceBreakpoints: list.map((b) => {
        nextBreakpointKey += 1;
        return {
          key: `bp-${nextBreakpointKey}`,
          file: b.file,
          line: b.line,
          enabled: b.enabled,
          address: null,
        };
      }),
    });
    get()._syncSourceBreakpoints();
  },

  setBreakpointsSink(sink) {
    set({ _breakpointsSink: sink });
  },

  _syncSourceBreakpoints() {
    const anchors = get().sourceBreakpoints;
    const provider = get()._provider;
    const resolve = resolver();
    if (!provider || !isDebugSessionProvider(provider) || resolve === null) {
      // Nothing to bind or arm against; anchors survive, bindings go null
      // so nothing renders confidently bound.
      if (anchors.some((a) => a.address !== null)) {
        set({ sourceBreakpoints: anchors.map((a) => ({ ...a, address: null })) });
      }
      return;
    }
    // The runtime set is derived state — re-arm from scratch so it can
    // never drift from the anchors (same posture as the resolvers: derived,
    // never stored).
    try {
      for (const armed of provider.debugBreakpoints()) {
        provider.debugBreakpointRemove(armed.id);
      }
    } catch {
      // A provider whose session vanished mid-sync; the mirror refresh
      // below reports whatever is really armed.
    }
    const next = anchors.map((a) => {
      const address = resolve(a.file, a.line);
      if (address !== null && a.enabled) {
        provider.debugBreakpointAdd(
          address.container_idx,
          address.offset,
          `${a.file}:${a.line + 1}`,
        );
      }
      const same =
        (address === null && a.address === null) ||
        (address !== null &&
          a.address !== null &&
          address.container_idx === a.address.container_idx &&
          address.offset === a.address.offset);
      return same ? a : { ...a, address };
    });
    let mirror: Breakpoint[] = [];
    try {
      mirror = provider.debugBreakpoints();
    } catch {
      mirror = [];
    }
    set({ sourceBreakpoints: next, debugBreakpoints: mirror });
  },
  // Mirrors the wasm session's own default, which is also true (W1/#3294).
  debugInfoEnabled: true,

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

  debugStepLine(mode, budgetCeiling) {
    // The author-tier step (W5/#3298, the granularity ladder's first
    // debug tier): one source line, bounded by armed breakpoints. This is
    // what the Player transport's Step Over/Into/Out drive; instruction
    // stepping (`debugStep`) is the Program Explorer's verb.
    const provider = get()._provider;
    if (!provider || !isDebugSessionProvider(provider)) return;
    const outcome = provider.debugStepLine(mode, budgetCeiling);
    set({ debugLastOutcome: outcome, debugStatus: statusOfOutcome(outcome) });
    get()._refreshDebugState();
  },

  debugRunToLine(budgetCeiling) {
    // Continue (2026-08-30 ruling — the granularity ladder's TOP tier):
    // deliver the next content line and resume play, bounded by armed
    // breakpoints. This is the Player transport's Continue; the statement
    // steps above are the programmer tier.
    const provider = get()._provider;
    if (!provider || !isDebugSessionProvider(provider)) return;
    const outcome = provider.debugRunToLine(budgetCeiling);
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
    // A bind/switch is exactly when the anchors need re-binding against
    // the (new) session's program — and a dispose is when their bindings
    // must go null (W4/#3297).
    get()._syncSourceBreakpoints();
  },
  };
};
