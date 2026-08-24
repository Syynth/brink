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
/** The editor's plain-text context menu (docs/editor-context-menu-spec.md):
 *  position + the text actions already bound to the raising view. The
 *  closures come from the editor (the only layer with the `EditorView`);
 *  the studio just renders and invokes. Structurally identical to
 *  `@brink-lang/editor`'s `TextMenuRequest` — kept independent so the store
 *  doesn't grow an editor dependency. */
export interface EditorTextMenuRequest {
  x: number;
  y: number;
  hasSelection: boolean;
  cut: () => void;
  copy: () => void;
  paste: () => void;
  selectAll: () => void;
  /** The clicked line's element kind (`todo`, `include`, …). */
  lineType?: string;
  /** Editor-side line-context items (Open File, Fold/Unfold, …). */
  lineActions?: { label: string; run: () => void }[];
  /** Navigate/Rename group for identity-bearing tokens (context-menu spec). */
  identity?: {
    name: string;
    gotoDefinition: () => void;
    findReferences?: () => void;
    rename?: () => void;
  };
}

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
  /** Open plain-text editor context menu, or null. */
  textMenu: EditorTextMenuRequest | null;
  /** Open the symbol context menu for a knot/stitch at a screen position. */
  openSymbolMenu(request: SymbolMenuRequest): void;
  openTextMenu(request: EditorTextMenuRequest): void;
  closeTextMenu(): void;
  /** Dismiss the symbol context menu. */
  closeSymbolMenu(): void;

  /** The open rename prompt, or null when closed. */
  renamePrompt: SymbolRenameRequest | null;
  /** Open the rename prompt for a knot/stitch. */
  openRenamePrompt(request: SymbolRenameRequest): void;
  /** Dismiss the rename prompt. */
  closeRenamePrompt(): void;

  /**
   * A human-readable description of the gated structural op currently
   * running off the paint path, or `null` when none is in flight. Started
   * as `moveStitch`/`promoteStitch`/`demoteKnot` only (#2767); #2776 reuses
   * the same field for the Binder's file/folder rename-and-move (`applyRename`,
   * `packages/studio-store/src/slices/binder.ts`) — both defer the same
   * shape of full-project breakage-gate call, so this is one generic
   * "a gated structural op is in flight" signal, not a symbol-menu-specific
   * one. This is a LOCAL busy-state affordance (spec §7.3), not a
   * notification: §7.5 states progress notifications are out of scope for
   * the notification service, so the pending state for these one-shot
   * actions renders in the status bar (`StructuralOpSegment`) instead of the
   * notification stack. Set synchronously by the caller (`runGatedStructuralOp`
   * in `packages/studio-ui/src/symbolMenuActions.ts`, or `applyRename` above)
   * before it defers the heavy call via `scheduleIdleWork`, and cleared once
   * that call settles.
   */
  structuralOpPending: string | null;
  /** Set the pending structural-op description (the start of a gated call).
   *  Takes only `string` (issue #2794 review) — clearing goes exclusively
   *  through {@link clearStructuralOpPending}'s compare-and-clear; no
   *  production or test caller passes `null` here, and narrowing the
   *  signature turns a future unconditional-clear regression into a
   *  `pnpm --filter @brink-lang/studio typecheck` failure instead of relying
   *  on review attention to catch it. */
  setStructuralOpPending(description: string): void;
  /**
   * Compare-and-clear (issue #2794): clears {@link structuralOpPending} only
   * if it still equals `description` — the exact string this call's own
   * `setStructuralOpPending` set. A no-op when another op's `set` has since
   * overwritten it, so a call that settles after being superseded cannot
   * erase the newer op's still-live indicator. TWO WRITERS share this field
   * (`runGatedStructuralOp` in `symbolMenuActions.ts`, and `applyRename` in
   * `binder.ts`) as independent fire-and-forget (`void`) dispatches — an
   * overlapping Binder drag-move and symbol-menu op is a real case, not a
   * hypothetical one. Every caller that commits a pending description before
   * deferring a gated call MUST clear through this, never through
   * {@link setStructuralOpPending} directly in a `finally` — that
   * unconditional-clear shape is exactly the last-writer-wins race #2794
   * fixed, and (as of this review) is no longer even typeable with `null`.
   */
  clearStructuralOpPending(description: string): void;
}

export const createSymbolMenuSlice: StateCreator<StudioState, [], [], SymbolMenuSlice> = (
  set,
  get,
) => ({
  symbolMenu: null,
  textMenu: null,
  openSymbolMenu(request) {
    // One menu at a time — a symbol menu replaces a text menu and vice versa.
    set({ symbolMenu: request, textMenu: null });
  },
  openTextMenu(request) {
    set({ textMenu: request, symbolMenu: null });
  },
  closeTextMenu() {
    set({ textMenu: null });
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
  clearStructuralOpPending(description) {
    // Compare-and-clear (#2794): only the writer whose own description is
    // still live may clear it — a superseded writer settling late must not
    // erase whatever op is live now.
    if (get().structuralOpPending === description) {
      set({ structuralOpPending: null });
    }
  },
});
