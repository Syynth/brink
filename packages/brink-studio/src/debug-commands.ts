/**
 * Debug session commands (D8's control bridge, #3232) — `debug.run` / `debug.pause` /
 * `debug.stepInto` / `debug.stepOver` / `debug.stepOut` (LINE steps since
 * W5/#3298 — the author tier) / `debug.
 * breakpointAdd` / `debug.breakpointRemove` / `debug.breakpointToggle`.
 *
 * Same discipline as `registerStoryCommands`: commands own the debug
 * session's mutation, gated by the bound provider's `"debug"` capability
 * (`session/types.ts`'s `DebugSessionProvider` extension) so the palette
 * simply has nothing to offer when the active provider doesn't support it
 * (a future remote provider, or before any session exists).
 *
 * Since W1 (#3294) every studio compile carries debug info, and since W5
 * (#3298) the Player's own transport dispatches these ids — they are live
 * surface, not dormant plumbing.
 *
 * Registered at the app boundary (main.tsx) and extracted here so the
 * gating is unit-testable without the bootstrap, mirroring
 * `story-commands.ts`.
 */

import type { CommandRegistry } from "@brink/studio-shell";
import type { StudioStore } from "@brink/studio-store";

/**
 * Register the `debug.*` commands against `store`. Returns a disposer that
 * unregisters all.
 */
export function registerDebugCommands(
  commands: CommandRegistry,
  store: StudioStore,
): () => void {
  const debugCapable = (): boolean => store.getState().debugCapable;

  const disposers = [
    commands.register({
      id: "debug.run",
      title: "Debug: Run",
      when: debugCapable,
      run: (args) => {
        const budgetCeiling = (args as { budgetCeiling?: number } | undefined)?.budgetCeiling;
        store.getState().debugRun(budgetCeiling);
      },
    }),

    commands.register({
      id: "debug.pause",
      title: "Debug: Pause",
      when: debugCapable,
      // The pause verb (W5/#3298, ruled first-class): enter the paused
      // state at the current boundary — the next reveal is a bounded line
      // step, the transport's step controls light up, debug.run resumes.
      run: () => store.getState().pauseSession(),
    }),

    commands.register({
      id: "debug.stepInto",
      title: "Debug: Step Into",
      when: debugCapable,
      run: () => store.getState().debugStepLine("into"),
    }),

    commands.register({
      id: "debug.stepOver",
      title: "Debug: Step Over",
      when: debugCapable,
      run: () => store.getState().debugStepLine("over"),
    }),

    commands.register({
      id: "debug.stepOut",
      title: "Debug: Step Out",
      when: debugCapable,
      run: () => store.getState().debugStepLine("out"),
    }),

    commands.register({
      id: "debug.breakpointAdd",
      title: "Debug: Add Breakpoint",
      when: debugCapable,
      run: (args) => {
        const o = args as
          | { containerIdx?: number; offset?: number; name?: string }
          | undefined;
        if (typeof o?.containerIdx === "number" && typeof o.offset === "number") {
          store.getState().debugBreakpointAdd(o.containerIdx, o.offset, o.name);
        }
      },
    }),

    commands.register({
      id: "debug.breakpointRemove",
      title: "Debug: Remove Breakpoint",
      when: debugCapable,
      run: (args) => {
        const id = typeof args === "number" ? args : (args as { id?: number } | undefined)?.id;
        if (typeof id === "number") store.getState().debugBreakpointRemove(id);
      },
    }),

    commands.register({
      id: "debug.breakpointToggle",
      title: "Debug: Toggle Breakpoint",
      when: debugCapable,
      run: (args) => {
        const o = args as { id?: number; enabled?: boolean } | undefined;
        if (typeof o?.id === "number" && typeof o.enabled === "boolean") {
          store.getState().debugBreakpointToggle(o.id, o.enabled);
        }
      },
    }),
  ];

  return () => {
    for (const dispose of disposers) dispose();
  };
}
