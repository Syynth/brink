/**
 * The single dispatcher for knot/stitch context-menu actions — "play from
 * here" plus the structural refactors (reorder / move / promote / demote).
 * Pure (takes the store state + `applyMoveResult`), so the Binder (incl. its
 * drag-drop), the editor, and the Story Graph all drive identical logic.
 */

import type { MoveResult } from "@brink/wasm-types";
import type { StudioState } from "@brink/studio-store";
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

  const session = state._project?.getSession();
  if (!session) return;

  let result: MoveResult;
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
