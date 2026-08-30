/**
 * Debug session commands (D8's control bridge, #3232) — `debug.run` /
 * `debug.continue` (2026-08-30 ruling: run to the next CONTENT line and
 * resume play — the transport's Continue) / `debug.pause` /
 * `debug.stepInto` / `debug.stepOver` / `debug.stepOut` (LINE steps since
 * W5/#3298 — the statement tier) / `debug.
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
  /** The focused ink document's path (F9 toggles at its cursor line);
   *  omitted by embedders without editor focus tracking. */
  getActiveInkFile?: () => string | null,
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
      id: "debug.continue",
      title: "Debug: Continue",
      keybinding: "F5",
      when: debugCapable,
      // Continue (2026-08-30 ruling): run until the next content line is
      // delivered — or a breakpoint/choices/terminal stop — and resume
      // play. The transport's Continue; `debug.run` stays the free-run.
      run: (args) => {
        const budgetCeiling = (args as { budgetCeiling?: number } | undefined)?.budgetCeiling;
        store.getState().debugRunToLine(budgetCeiling);
      },
    }),

    commands.register({
      id: "debug.pause",
      title: "Debug: Pause",
      keybinding: "F6",
      when: debugCapable,
      // The pause verb (W5/#3298, ruled first-class): enter the paused
      // state at the current boundary — the step controls light up, and
      // debug.continue delivers the next content line and resumes.
      run: () => store.getState().pauseSession(),
    }),

    commands.register({
      id: "debug.stepInto",
      title: "Debug: Step Into",
      keybinding: "F11",
      when: debugCapable,
      run: () => store.getState().debugStepLine("into"),
    }),

    commands.register({
      id: "debug.stepOver",
      title: "Debug: Step Over",
      keybinding: "F10",
      when: debugCapable,
      run: () => store.getState().debugStepLine("over"),
    }),

    commands.register({
      id: "debug.stepOut",
      title: "Debug: Step Out",
      keybinding: "Shift-F11",
      when: debugCapable,
      run: () => store.getState().debugStepLine("out"),
    }),

    commands.register({
      id: "debug.toggleBreakpoint",
      title: "Debug: Toggle Breakpoint (Current Line)",
      keybinding: "F9",
      // Anchors are source-anchored (W4) — they exist without a session,
      // so this is gated on knowing WHERE, not on debug capability.
      when: () => getActiveInkFile?.() != null,
      run: () => {
        const file = getActiveInkFile?.();
        if (file == null) return;
        // Store cursor is 1-based; anchors are 0-based.
        store.getState().breakpointToggleAtLine(file, store.getState().cursor.line - 1);
      },
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
