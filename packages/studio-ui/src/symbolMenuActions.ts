/**
 * The single dispatcher for knot/stitch context-menu actions — "play from
 * here" plus the structural refactors (reorder / move / promote / demote).
 * Pure (takes the store state + `applyMoveResult`), so the Binder (incl. its
 * drag-drop), the editor, and the Story Graph all drive identical logic.
 */

import type { StructuralResult, RenameDiagnostic } from "@brink/wasm-types";
import type { StudioState, SymbolRenameRequest } from "@brink/studio-store";
import type { ContextMenuAction } from "./BinderContextMenu.js";

export async function dispatchSymbolAction(
  state: StudioState,
  applyMoveResult: StudioState["applyMoveResult"],
  action: ContextMenuAction,
): Promise<void> {
  if (action.type === "playFromHere") {
    state.openSession({ path: action.inkPath, label: action.label });
    return;
  }

  // Rename opens an interactive prompt (name → breakage report); the rename
  // itself runs from the prompt via `performSymbolRename` (#305).
  if (action.type === "renameSymbol") {
    state.openRenamePrompt({
      path: action.path,
      knot: action.knot,
      stitch: action.stitch,
      currentName: action.stitch ?? action.knot,
    });
    return;
  }

  const session = state._project?.getSession();
  if (!session) return;

  let result: StructuralResult;
  let description: string;
  switch (action.type) {
    case "reorderStitch":
      result = session.reorderStitch(action.path, action.knot, action.stitch, action.direction);
      description = `Reorder ${action.stitch} ${action.direction > 0 ? "down" : "up"}`;
      break;
    case "reorderKnot":
      result = session.reorderKnot(action.path, action.knot, action.direction);
      description = `Reorder ${action.knot} ${action.direction > 0 ? "down" : "up"}`;
      break;
    case "reorderStitches":
      result = session.reorderStitches(action.path, action.knot, action.order);
      description = `Reorder stitches in ${action.knot}`;
      break;
    case "reorderKnots":
      result = session.reorderKnots(action.path, action.order);
      description = `Reorder knots`;
      break;
    case "moveStitch":
      result = session.moveStitch(action.path, action.srcKnot, action.stitch, action.destKnot);
      description = `Move ${action.stitch} to ${action.destKnot}`;
      break;
    case "promoteStitch":
      result = session.promoteStitch(action.path, action.knot, action.stitch);
      description = `Promote ${action.stitch} to knot`;
      break;
    case "demoteKnot":
      result = session.demoteKnot(action.path, action.knot, action.destKnot);
      description = `Demote ${action.knot} into ${action.destKnot}`;
      break;
    default:
      // File/folder actions are owned by the Binder and never reach here.
      return;
  }

  if (result.ok && result.path) {
    await applyMoveResult(result, description, [result.path]);
  }
}

/** The outcome of attempting a symbol rename. */
export interface SymbolRenameOutcome {
  /** True when the edits were applied (safe, or forced). */
  applied: boolean;
  /** The diagnostics the rename would introduce (the breakage report). */
  diagnostics: RenameDiagnostic[];
  /** An error from the rename op (symbol vanished, etc.), if any. */
  error?: string;
}

/**
 * Run a knot/stitch rename, safe-by-default (#305). Computes the rename and its
 * introduced-diagnostic breakage report; applies the edits when the rename is
 * safe or `force` is set, otherwise returns the report so the prompt can show
 * it. Used by `SymbolRenamePrompt`.
 */
export async function performSymbolRename(
  state: StudioState,
  applyMoveResult: StudioState["applyMoveResult"],
  req: SymbolRenameRequest,
  newName: string,
  force: boolean,
): Promise<SymbolRenameOutcome> {
  const session = state._project?.getSession();
  if (!session) return { applied: false, diagnostics: [] };

  // Offset-based (F2) covers any symbol under the cursor; name-based (menu)
  // targets a knot/stitch. Both return the same safe-rename payload.
  const result =
    req.offset != null
      ? session.renameSymbolAt(req.path, req.offset, newName)
      : session.renameSymbol(req.path, req.knot ?? "", req.stitch ?? "", newName);
  if (!result.ok) {
    return { applied: false, diagnostics: [], error: result.error };
  }

  if (result.safe || force) {
    const oldName = req.currentName ?? req.stitch ?? req.knot;
    const label = req.currentName ?? (req.stitch ? `${req.knot}.${req.stitch}` : req.knot) ?? "symbol";
    await applyMoveResult(
      result,
      `Rename ${label} to ${newName}`,
      result.path ? [result.path] : [],
    );
    // Keep an open symbol view of the renamed knot/stitch aligned: re-key its
    // `path::oldName` tab to `path::newName` in place (#305).
    if (oldName !== undefined && oldName !== newName) {
      state.renameSymbolDocKey(req.path, oldName, newName);
    }
    return { applied: true, diagnostics: result.introduced_diagnostics };
  }

  return { applied: false, diagnostics: result.introduced_diagnostics };
}
