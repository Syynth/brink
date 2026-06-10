/**
 * Player slice — player *UI* state only.
 *
 * The story session itself (runner handle, transcript, choices, debug state,
 * choice history, status) is a first-class model in its own slice
 * (session.ts, docs/studio-shell-spec.md §7.6). What stays here is genuinely
 * presentation: the player's fullscreen mode. The two have different
 * lifetimes — restarting a story must not reset the user's layout, and
 * toggling fullscreen must not touch the VM.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";

export interface PlayerSlice {
  /** Player fullscreen mode — the player tool window covers the shell. */
  playerFullscreen: boolean;
  togglePlayerFullscreen(): void;
}

export const createPlayerSlice: StateCreator<StudioState, [], [], PlayerSlice> = (set) => ({
  playerFullscreen: false,

  togglePlayerFullscreen() {
    set((state) => ({ playerFullscreen: !state.playerFullscreen }));
  },
});
