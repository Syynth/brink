/**
 * The store subscription that drives the editors' debug adornments
 * (W4/#3297 breakpoint dots, W6/#3299 execution highlight + reveal-on-
 * stop) — extracted from `mount.tsx` (the `location-resolvers.ts`
 * pattern) because its re-entrancy discipline needs a test of its own.
 *
 * The editors re-read only on these refreshes — no polling. Reveal-on-
 * stop rides the paused rising edge, through the W3 resolver chain
 * (degraded-gated there).
 *
 * ── Re-entrancy ──────────────────────────────────────────────────────
 * `_syncSourceBreakpoints` and the reveal dispatch call `setState`, and
 * zustand notifies subscribers SYNCHRONOUSLY — this listener re-enters
 * mid-flight. Every change flag is therefore derived FIRST and `last`
 * reassigned BEFORE any side effect runs: a stale `last` re-derives the
 * same flags forever (a real boot-time stack overflow, found live —
 * swallowed by the provider's load-error catch, so the only symptom was
 * "Load error: Maximum call stack size exceeded" in the Player).
 */

import type { StudioStore } from "@brink/studio-store";

export interface DebugRefreshTargets {
  /** Re-render the breakpoint gutter dots in every open editor. */
  refreshBreakpoints(): void;
  /** Re-render the execution highlight in every open editor. */
  refreshExecutionHighlight(): void;
  /** Reveal a program address in the editor (the W3 resolver chain). */
  revealProgram(containerIdx: number, offset: number): void;
  /** Follow (#3437): scroll the editor to a revealed line's source without
   *  taking focus. Optional so older hosts/tests need not provide it. */
  followSource?(source: { file: string; range_start: number; range_end: number }): void;
}

/** Subscribe `store` to drive `targets`; returns the unsubscribe. */
export function subscribeDebugRefresh(
  store: StudioStore,
  targets: DebugRefreshTargets,
): () => void {
  let last = snapshot(store);
  return store.subscribe((st) => {
    const anchorsChanged =
      st.sourceBreakpoints !== last.anchors ||
      st.programChecksum !== last.program ||
      st.compiledChecksum !== last.compiled;
    const programChanged = st.programChecksum !== last.program;
    const highlightMoved =
      st.debugState !== last.debugState ||
      st.sessionPaused !== last.paused ||
      st.sessionStatus !== last.status ||
      st.programChecksum !== last.program ||
      st.compiledChecksum !== last.compiled ||
      // W8/#3301: a frame selection draws/clears the accent frame band.
      st.selectedFrameIdx !== last.frameIdx;
    const pausedRose = st.sessionPaused && !last.paused;
    // Follow / hover (#3437).
    const linesMoved = st.sessionLines !== last.lines;
    const hoverMoved = st.sessionHoverSource !== last.hover || st.sessionPeek !== last.peek;
    const followFlipped =
      st.followInEditor !== last.follow || st.followPaused !== last.followPaused;
    const sessionStarted = last.status === "none" && st.sessionStatus !== "none";
    last = {
      anchors: st.sourceBreakpoints,
      program: st.programChecksum,
      compiled: st.compiledChecksum,
      debugState: st.debugState,
      paused: st.sessionPaused,
      status: st.sessionStatus,
      frameIdx: st.selectedFrameIdx,
      lines: st.sessionLines,
      hover: st.sessionHoverSource,
      peek: st.sessionPeek,
      follow: st.followInEditor,
      followPaused: st.followPaused,
    };
    // A new run lifts the pause an edit put on follow.
    if (sessionStarted && st.followPaused) store.getState().setFollowPaused(false);

    if (anchorsChanged) {
      // A running-program identity change means the provider swapped or
      // reloaded its internal session — the runtime breakpoint set must
      // re-arm from the anchors (belt to the slice-level braces in
      // startSession/openSession, and the net for provider-internal
      // reloads the slice never sees).
      if (programChanged) store.getState()._syncSourceBreakpoints();
      targets.refreshBreakpoints();
    }
    if (highlightMoved || hoverMoved || followFlipped || (linesMoved && st.followInEditor)) {
      targets.refreshExecutionHighlight();
    }
    if (pausedRose) {
      const pos = st.debugState?.position;
      if (pos) targets.revealProgram(pos.container_idx, pos.offset);
    }
    // Follow (#3437): each newly revealed line scrolls the editor to its
    // source — while playing, follow on, not paused by an edit, and not
    // at a debugger pause (that road reveals with focus, above).
    const playing = st.sessionStatus === "running" || st.sessionStatus === "awaiting-choice";
    if (
      linesMoved &&
      playing &&
      st.followInEditor &&
      !st.followPaused &&
      !st.sessionPaused &&
      targets.followSource
    ) {
      for (let i = st.sessionLines.length - 1; i >= 0; i--) {
        const src = st.sessionLines[i]?.source;
        if (src) {
          targets.followSource(src);
          break;
        }
      }
    }
  });
}

function snapshot(store: StudioStore) {
  const st = store.getState();
  return {
    anchors: st.sourceBreakpoints,
    program: st.programChecksum,
    compiled: st.compiledChecksum,
    debugState: st.debugState,
    paused: st.sessionPaused,
    status: st.sessionStatus,
    frameIdx: st.selectedFrameIdx,
    lines: st.sessionLines,
    hover: st.sessionHoverSource,
    peek: st.sessionPeek,
    follow: st.followInEditor,
    followPaused: st.followPaused,
  };
}
