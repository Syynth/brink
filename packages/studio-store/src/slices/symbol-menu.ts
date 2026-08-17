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
  /** Which surface raised the menu. Editor-origin renames run inline in the
   *  editor (#323/#324); graph-origin renames use the modal prompt. */
  source?: "editor" | "graph";
}

/** A pending request to rename a symbol. Holds only primitives; the prompt
 * component drives the safe-by-default flow (#305). Two seedings:
 *  - **name-based** (context menu): `knot` (+ optional `stitch`).
 *  - **offset-based** (editor F2): `offset` (a whole-file UTF-16 offset),
 *    covering any symbol under the cursor, not just knots/stitches.
 * `currentName` seeds the input (defaults to `stitch ?? knot`). */
export interface SymbolRenameRequest {
  /** Project-relative file path of the symbol's declaration. */
  path: string;
  /** The knot name (the stitch's parent knot, for a stitch). Name-based seed. */
  knot?: string;
  /** The stitch name, when the target is a stitch. */
  stitch?: string;
  /** Whole-file UTF-16 offset of the symbol under the cursor. Offset-based seed (F2). */
  offset?: number;
  /** The symbol's current name, used to pre-fill the input. */
  currentName?: string;
}

export interface SymbolMenuSlice {
  /** The open symbol context-menu request, or null when closed. */
  symbolMenu: SymbolMenuRequest | null;
  /** Open the symbol context menu for a knot/stitch at a screen position. */
  openSymbolMenu(request: SymbolMenuRequest): void;
  /** Dismiss the symbol context menu. */
  closeSymbolMenu(): void;

  /** The open rename prompt, or null when closed. */
  renamePrompt: SymbolRenameRequest | null;
  /** Open the rename prompt for a knot/stitch. */
  openRenamePrompt(request: SymbolRenameRequest): void;
  /** Dismiss the rename prompt. */
  closeRenamePrompt(): void;

  /**
   * A human-readable description of the gated structural op (`moveStitch`/
   * `promoteStitch`/`demoteKnot`, #2767) currently running off the paint
   * path, or `null` when none is in flight. This is a LOCAL busy-state
   * affordance (spec §7.3), not a notification: §7.5 states progress
   * notifications are out of scope for the notification service, so the
   * pending state for these one-shot context-menu/drag-drop actions renders
   * in the status bar (`StructuralOpSegment`) instead of the notification
   * stack. Set synchronously by `runGatedStructuralOp`
   * (`packages/studio-ui/src/symbolMenuActions.ts`) before it defers the
   * heavy call via `scheduleIdleWork`, and cleared once that call settles.
   */
  structuralOpPending: string | null;
  /** Set (or clear, with `null`) the pending structural-op description. */
  setStructuralOpPending(description: string | null): void;
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

  renamePrompt: null,
  openRenamePrompt(request) {
    set({ renamePrompt: request });
  },
  closeRenamePrompt() {
    set({ renamePrompt: null });
  },

  structuralOpPending: null,
  setStructuralOpPending(description) {
    set({ structuralOpPending: description });
  },
});
