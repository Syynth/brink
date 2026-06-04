/**
 * Layout slice — responsive layout state.
 *
 * `tier` is driven by a ResizeObserver on the studio root (see
 * studio-ui/useTier). Below `wide`, the binder becomes a slide-over drawer; in
 * `narrow`, the editor/player become an Editor·Story tab pair (player overlay).
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";

export type LayoutTier = "wide" | "medium" | "narrow";

/** Which view occupies the left sidebar dock (selected via the activity bar). */
export type SidebarView = "binder" | "state";

export interface LayoutSlice {
  /** Current responsive tier (set by the width observer). */
  tier: LayoutTier;
  /** Compact tiers: whether the binder drawer is open. */
  binderDrawerOpen: boolean;
  /** Narrow tier: whether the Story (player) overlay is active vs the editor. */
  storyOpen: boolean;
  /** The view shown in the left sidebar dock / drawer. */
  activeSidebarView: SidebarView;

  setTier(tier: LayoutTier): void;
  setBinderDrawerOpen(open: boolean): void;
  toggleBinderDrawer(): void;
  setStoryOpen(open: boolean): void;
  toggleStory(): void;
  setSidebarView(view: SidebarView): void;
}

export const createLayoutSlice: StateCreator<StudioState, [], [], LayoutSlice> = (set) => ({
  tier: "wide",
  binderDrawerOpen: false,
  storyOpen: false,
  activeSidebarView: "binder",

  setTier(tier) {
    set((s) => {
      if (s.tier === tier) return {};
      // Entering a new tier dismisses transient overlays so we never land in a
      // stuck state (e.g. drawer open after expanding back to wide).
      return {
        tier,
        binderDrawerOpen: false,
        storyOpen: tier === "narrow" ? s.storyOpen : false,
      };
    });
  },

  setBinderDrawerOpen(open) {
    set({ binderDrawerOpen: open });
  },
  toggleBinderDrawer() {
    set((s) => ({ binderDrawerOpen: !s.binderDrawerOpen }));
  },
  setStoryOpen(open) {
    set({ storyOpen: open });
  },
  toggleStory() {
    set((s) => ({ storyOpen: !s.storyOpen }));
  },
  setSidebarView(view) {
    set({ activeSidebarView: view });
  },
});
