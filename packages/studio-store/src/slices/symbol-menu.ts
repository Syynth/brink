/**
 * Symbol context-menu slice — the transport for the shared knot/stitch
 * context menu (#186 follow-up). The editor (CM6) and the Story Graph raise a
 * request here; a single `SymbolContextMenuHost` in the studio-ui tree renders
 * the menu. Holds only primitives so studio-store stays free of UI types — the
 * host rebuilds the full menu target from the outline.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";

/** A pending request to open the symbol context menu at a screen position. */
export interface SymbolMenuRequest {
  /** Project-relative file path of the symbol's declaration. */
  path: string;
  /** The knot name (the stitch's parent knot, for a stitch). */
  knot: string;
  /** The stitch name, when the target is a stitch. */
  stitch?: string;
  /** Viewport coordinates of the click. */
  x: number;
  y: number;
}

export interface SymbolMenuSlice {
  /** The open symbol context-menu request, or null when closed. */
  symbolMenu: SymbolMenuRequest | null;
  /** Open the symbol context menu for a knot/stitch at a screen position. */
  openSymbolMenu(request: SymbolMenuRequest): void;
  /** Dismiss the symbol context menu. */
  closeSymbolMenu(): void;
}

export const createSymbolMenuSlice: StateCreator<StudioState, [], [], SymbolMenuSlice> = (
  set,
) => ({
  symbolMenu: null,
  openSymbolMenu(request) {
    set({ symbolMenu: request });
  },
  closeSymbolMenu() {
    set({ symbolMenu: null });
  },
});
