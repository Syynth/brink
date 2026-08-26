/**
 * @brink/studio-shell — shell layout store (docs/studio-shell-spec.md §7.1).
 *
 * The ShellLayoutSlice replacing studio-store's old LayoutSlice: tier,
 * per-window placements, open occupant per dock section, dock sizes, and
 * the maximize placeholder (#86). Plus transient compact-tier presentation
 * (drawer visibility, narrow overlay selection) which is reset on tier
 * changes, mirroring the old LayoutSlice.setTier hygiene.
 *
 * Persistence is deliberately absent (issue #88); layouts reset on reload.
 * Created inside ShellProvider as a vanilla store — components read it via
 * useShellLayout(selector).
 */

import { createStore, type StoreApi } from "zustand/vanilla";
import {
  DOCK_SECTION_IDS,
  dockSectionId,
  type Dock,
  type DockSectionId,
  type Placement,
  type Section,
  type ToolWindowDescriptor,
} from "./toolwindow.js";

/** Responsive presentation tier (spec §5.3), driven by the width observer. */
export type LayoutTier = "wide" | "medium" | "narrow";

/**
 * The editor root area's possible occupants. "code" is today's tabbed
 * surface with groups and splits; "single" shows one file with the host's
 * companion document beside it. "continuous" joins them later.
 */
export type EditorViewId = "code" | "single";

export interface ShellLayoutState {
  /** Current responsive tier. */
  tier: LayoutTier;
  /** dock+section per tool window, seeded from descriptor defaults. */
  placements: Record<string, Placement>;
  /** Current occupant per dock section (at most one open per section). */
  open: Record<DockSectionId, string | null>;
  /** Last dock sizes in px — restored when a collapsed dock reopens. */
  dockSizes: Record<Dock, number>;
  /** Maximized tool window: covers the editor area until restored (§5.4). */
  maximized: string | null;

  /**
   * Which view occupies the editor root area (decision log 2026-08-26,
   * "The editor root area has one occupant"). The area holds exactly one
   * thing: a view over the project's files, and later the Story Graph,
   * which takes the area over rather than opening inside a view.
   *
   * A preference about how you write rather than a fact about the project,
   * so it persists with the rest of the layout (globally) rather than
   * per-project alongside each view's own remembered state.
   */
  editorView: EditorViewId;

  // ── Transient compact-tier presentation (reset on tier change) ──
  /** medium/narrow: slide-over drawer visibility per side dock. */
  drawers: Record<"left" | "right", boolean>;
  /** narrow: which open (non-left) tool window covers the editor. */
  narrowView: string | null;

  /**
   * Reconcile with the tool-window registry: new ids get their default
   * placement (and defaultOpen seeds an empty section — first registered
   * wins); removed ids are dropped from placements/open; user state for
   * surviving ids is preserved.
   */
  syncFromRegistry(descriptors: readonly ToolWindowDescriptor[]): void;
  /**
   * Toggle a tool window: open it in its section (closing the section's
   * previous occupant) or close it. In compact tiers, toggling an open but
   * hidden window reveals it (drawer / narrow overlay) instead of closing.
   */
  toggleToolWindow(id: string): void;
  /** Choose the editor root area's occupant. */
  setEditorView(view: EditorViewId): void;
  /** Re-dock a tool window; if it was open, it opens in the new section. */
  moveToolWindow(id: string, dock: Dock, section: Section): void;
  /** Maximize a tool window over the editor area, or restore it (§5.4). */
  toggleMaximize(id: string): void;
  /** Remember a dock's size (px) so reopening restores it. */
  setDockSize(dock: Dock, px: number): void;
  /** Set the responsive tier; dismisses transient overlay/drawer state. */
  setTier(tier: LayoutTier): void;
  /** medium/narrow: show/hide a side dock's slide-over drawer. */
  setDrawerOpen(side: "left" | "right", open: boolean): void;
  /** Hide both drawers (scrim click). */
  closeDrawers(): void;
  /** narrow: pick the tool window shown as a full overlay (null = editor). */
  setNarrowView(id: string | null): void;
}

export type ShellLayoutStore = StoreApi<ShellLayoutState>;

function emptyOpen(): Record<DockSectionId, string | null> {
  return {
    "left.start": null,
    "left.end": null,
    "right.start": null,
    "right.end": null,
    "bottom.start": null,
    "bottom.end": null,
  };
}

/** Is `id` the open occupant of its placed section? */
export function isToolWindowOpen(state: ShellLayoutState, id: string): boolean {
  const placement = state.placements[id];
  return placement !== undefined && state.open[dockSectionId(placement)] === id;
}

/**
 * Ensure a tool window is open *and visible* — the imperative complement to
 * `toggleToolWindow` for commands like `search.focus` that must never close
 * the window (a toggle on an open, visible window would). Closed windows are
 * opened via the toggle (which also reveals in compact tiers); open-but-
 * hidden windows get their compact presentation (drawer / narrow overlay)
 * surfaced directly; open-and-visible windows are left alone.
 */
export function ensureToolWindowOpen(store: ShellLayoutStore, id: string): void {
  const state = store.getState();
  const placement = state.placements[id];
  if (placement === undefined) return;

  if (!isToolWindowOpen(state, id)) {
    state.toggleToolWindow(id);
    return;
  }

  // Already open. Surface the compact presentations that may be hiding it
  // (mirrors toggleToolWindow's reveal rules — narrow: left dock stays a
  // drawer, right/bottom present as overlays).
  if (state.tier === "narrow" && placement.dock !== "left") {
    if (state.narrowView !== id) state.setNarrowView(id);
    return;
  }
  const usesDrawer =
    (state.tier === "medium" && placement.dock !== "bottom") ||
    (state.tier === "narrow" && placement.dock === "left");
  if (usesDrawer) {
    const side = placement.dock as "left" | "right";
    if (!state.drawers[side]) state.setDrawerOpen(side, true);
  }
}

export function createShellLayoutStore(): ShellLayoutStore {
  return createStore<ShellLayoutState>()((set, get) => ({
    tier: "wide",
    placements: {},
    open: emptyOpen(),
    dockSizes: { left: 220, right: 300, bottom: 180 },
    maximized: null,
    editorView: "code",
    drawers: { left: false, right: false },
    narrowView: null,

    syncFromRegistry(descriptors) {
      set((s) => {
        const known = new Set(descriptors.map((d) => d.id));

        // Placements: keep the user's for surviving ids, default new ids.
        // Iteration follows registration order — deterministic.
        const placements: Record<string, Placement> = {};
        for (const d of descriptors) {
          placements[d.id] = s.placements[d.id] ?? d.defaultPlacement;
        }

        // Open: keep occupants that survive and still live in that section.
        const open = emptyOpen();
        for (const key of DOCK_SECTION_IDS) {
          const occupant = s.open[key];
          if (
            occupant !== null &&
            known.has(occupant) &&
            dockSectionId(placements[occupant]) === key
          ) {
            open[key] = occupant;
          }
        }

        // defaultOpen seeds only newly-seen ids, and only an empty section —
        // the first registered defaultOpen view per section wins. A window
        // the user closed stays closed across registry changes.
        for (const d of descriptors) {
          if (s.placements[d.id] === undefined && d.defaultOpen) {
            const key = dockSectionId(placements[d.id]);
            if (open[key] === null) open[key] = d.id;
          }
        }

        const narrowView =
          s.narrowView !== null && known.has(s.narrowView) ? s.narrowView : null;
        // A persisted maximized id whose window no longer exists would leave
        // the shell stuck half-restored (Escape targets a ghost id).
        const maximized =
          s.maximized !== null && known.has(s.maximized) ? s.maximized : null;
        return { placements, open, narrowView, maximized };
      });
    },

    setEditorView(view) {
      set({ editorView: view });
    },

    toggleToolWindow(id) {
      set((s) => {
        const placement = s.placements[id];
        if (placement === undefined) return {};
        const key = dockSectionId(placement);
        const compactSide = placement.dock !== "bottom";
        // narrow: left dock stays a drawer; right/bottom present as overlays.
        const usesNarrowOverlay =
          s.tier === "narrow" && placement.dock !== "left";
        const usesDrawer =
          (s.tier === "medium" && compactSide) ||
          (s.tier === "narrow" && placement.dock === "left");

        const reveal = (): Partial<ShellLayoutState> => {
          if (usesNarrowOverlay) return { narrowView: id };
          if (usesDrawer) {
            const side = placement.dock as "left" | "right";
            return { drawers: { ...s.drawers, [side]: true } };
          }
          return {};
        };

        if (s.open[key] !== id) {
          // Open (closing the section's previous occupant), and reveal in
          // compact tiers so the toggle is never a visual no-op.
          return { open: { ...s.open, [key]: id }, ...reveal() };
        }

        // Already open. If a compact presentation is hiding it, reveal
        // instead of closing (a strip click should surface the window).
        const hidden =
          (usesNarrowOverlay && s.narrowView !== id) ||
          (usesDrawer && !s.drawers[placement.dock as "left" | "right"]);
        if (hidden) return reveal();

        // Close.
        const next: Partial<ShellLayoutState> = { open: { ...s.open, [key]: null } };
        if (s.narrowView === id) next.narrowView = null;
        if (usesDrawer) {
          const side = placement.dock as "left" | "right";
          const otherKey = dockSectionId({
            dock: placement.dock,
            section: placement.section === "start" ? "end" : "start",
          });
          if (s.open[otherKey] === null) {
            next.drawers = { ...s.drawers, [side]: false };
          }
        }
        return next;
      });
    },

    moveToolWindow(id, dock, section) {
      set((s) => {
        const placement = s.placements[id];
        if (placement === undefined) return {};
        if (placement.dock === dock && placement.section === section) return {};

        const fromKey = dockSectionId(placement);
        const toKey = dockSectionId({ dock, section });
        const placements = { ...s.placements, [id]: { dock, section } };
        const open = { ...s.open };
        if (open[fromKey] === id) {
          // Stay open in the new home, displacing its previous occupant.
          open[fromKey] = null;
          open[toKey] = id;
        }
        return { placements, open };
      });
    },

    toggleMaximize(id) {
      set((s) => {
        if (s.maximized === id) return { maximized: null };
        if (s.placements[id] === undefined) return {};
        return { maximized: id };
      });
    },

    setDockSize(dock, px) {
      if (!Number.isFinite(px) || px <= 0) return;
      const rounded = Math.round(px);
      if (get().dockSizes[dock] === rounded) return;
      set((s) => ({ dockSizes: { ...s.dockSizes, [dock]: rounded } }));
    },

    setTier(tier) {
      set((s) => {
        if (s.tier === tier) return {};
        // Entering a new tier dismisses transient presentation so we never
        // land in a stuck state (e.g. drawer open after expanding to wide).
        return { tier, drawers: { left: false, right: false }, narrowView: null };
      });
    },

    setDrawerOpen(side, open) {
      set((s) =>
        s.drawers[side] === open ? {} : { drawers: { ...s.drawers, [side]: open } },
      );
    },

    closeDrawers() {
      set((s) =>
        s.drawers.left || s.drawers.right
          ? { drawers: { left: false, right: false } }
          : {},
      );
    },

    setNarrowView(id) {
      set({ narrowView: id });
    },
  }));
}
