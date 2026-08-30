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
      st.compiledChecksum !== last.compiled;
    const pausedRose = st.sessionPaused && !last.paused;
    last = {
      anchors: st.sourceBreakpoints,
      program: st.programChecksum,
      compiled: st.compiledChecksum,
      debugState: st.debugState,
      paused: st.sessionPaused,
      status: st.sessionStatus,
    };

    if (anchorsChanged) {
      // A running-program identity change means the provider swapped or
      // reloaded its internal session — the runtime breakpoint set must
      // re-arm from the anchors (belt to the slice-level braces in
      // startSession/openSession, and the net for provider-internal
      // reloads the slice never sees).
      if (programChanged) store.getState()._syncSourceBreakpoints();
      targets.refreshBreakpoints();
    }
    if (highlightMoved) {
      targets.refreshExecutionHighlight();
    }
    if (pausedRose) {
      const pos = st.debugState?.position;
      if (pos) targets.revealProgram(pos.container_idx, pos.offset);
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
  };
}
