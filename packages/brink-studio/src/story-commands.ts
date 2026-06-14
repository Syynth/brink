/**
 * Story session lifecycle commands (docs/studio-shell-spec.md §7.6, §6;
 * docs/live-inspector-spec.md §4).
 *
 * Commands own session mutation: views (PlayerPane, StateView, …) dispatch
 * these by id and never call the session slice's actions directly. The `when`
 * predicates close over the store and gate by session status — they also
 * drive palette enablement (and, later, strip badges and status-bar state).
 *
 * Each drive verb also gates on the bound provider's **capabilities** (spec
 * §4): a verb the provider doesn't advertise fails its `when`, so the command
 * vanishes from the palette/strips/headers with no per-view branching. The
 * local provider advertises the full set, so this is invisible today; it lets
 * an observe-only remote provider render every view read-only.
 *
 * Registered at the app boundary (main.tsx) and extracted here so the gating
 * is unit-testable without the bootstrap.
 */

import type { CommandRegistry } from "@brink/studio-shell";
import type { StudioStore, SessionCapability } from "@brink/studio-store";
import { sessionCanContinue } from "@brink/studio-store";

/**
 * Register `story.start` / `story.restart` / `story.stop` / `story.choose` /
 * `story.continue` against `store`. Returns a disposer that unregisters all.
 */
export function registerStoryCommands(
  commands: CommandRegistry,
  store: StudioStore,
): () => void {
  // The program a session can (re)start on: the latest successful compile,
  // falling back to the current session's own program — a failed compile
  // nulls `storyBytes` but must not strand the session (spec §7.6).
  const programBytes = (): Uint8Array | null => {
    const state = store.getState();
    return state.storyBytes ?? state._sessionBytes;
  };

  // Whether the bound provider advertises a drive verb (spec §3.2/§4). Defaults
  // to the full local set before any provider binds, so a local session is
  // always startable.
  const can = (cap: SessionCapability): boolean =>
    store.getState().capabilities.has(cap);

  const disposers = [
    commands.register({
      id: "story.start",
      title: "Story: Start",
      when: () =>
        store.getState().sessionStatus === "none" &&
        programBytes() !== null &&
        can("start"),
      run: () => {
        const bytes = programBytes();
        if (bytes) store.getState().startSession(bytes);
      },
    }),

    commands.register({
      id: "story.restart",
      title: "Story: Restart",
      when: () =>
        (store.getState().sessionStatus !== "none" || programBytes() !== null) &&
        can("restart"),
      // `restartSession` resets a live runner in place, or — with no live
      // runner (status "none", or "error" from a failed load) — starts fresh
      // on the available program. The provider/runner distinction lives in the
      // slice now (spec §3); the command no longer reaches into session refs.
      run: () => store.getState().restartSession(),
    }),

    commands.register({
      id: "story.stop",
      title: "Story: Stop",
      when: () => store.getState().sessionStatus !== "none" && can("stop"),
      run: () => store.getState().stopSession(),
    }),

    commands.register({
      id: "story.choose",
      title: "Story: Choose",
      when: () =>
        store.getState().sessionStatus === "awaiting-choice" && can("choose"),
      run: (args) => {
        const index =
          typeof args === "number"
            ? args
            : (args as { index?: number } | undefined)?.index;
        if (typeof index === "number") store.getState().chooseOption(index);
      },
    }),

    commands.register({
      id: "story.continue",
      title: "Story: Continue",
      when: () =>
        sessionCanContinue(store.getState().sessionStatus) && can("continue"),
      run: () => store.getState().revealNext(),
    }),
  ];

  return () => {
    for (const dispose of disposers) dispose();
  };
}
