/**
 * Renders the shared knot/stitch context menu for non-Binder surfaces (the
 * editor and the Story Graph). They raise a request via the store's
 * `openSymbolMenu`; this single host — mounted once near the studio root —
 * rebuilds the full menu target from the outline and renders the same
 * `BinderContextMenu` + `useSymbolMenuActions` the Binder uses. One menu,
 * one dispatcher, everywhere.
 */

import type { ReactElement } from "react";
import type { FileOutline } from "@brink/wasm-types";
import type { SymbolMenuRequest } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";
import {
  BinderContextMenu,
  type ContextMenuAction,
  type ContextMenuTarget,
} from "./BinderContextMenu.js";
import { useSymbolMenuActions } from "./useSymbolMenuActions.js";

/** Resolve a `{path, knot, stitch?}` request to a full menu target (with the
 *  sibling index/count the reorder items need) from the project outline. */
function buildSymbolTarget(
  outline: FileOutline[],
  req: SymbolMenuRequest,
): ContextMenuTarget | null {
  const file = outline.find((f) => f.path === req.path);
  if (!file) return null;
  const knots = file.symbols.filter((s) => s.kind === "knot");

  if (req.stitch !== undefined) {
    const knot = knots.find((k) => k.name === req.knot);
    if (!knot) return null;
    const stitches = knot.children.filter((c) => c.kind === "stitch");
    const index = stitches.findIndex((s) => s.name === req.stitch);
    if (index < 0) return null;
    return {
      kind: "stitch",
      path: req.path,
      knot: req.knot,
      stitch: req.stitch,
      index,
      siblingCount: stitches.length,
    };
  }

  const index = knots.findIndex((k) => k.name === req.knot);
  if (index < 0) return null;
  return { kind: "knot", path: req.path, knot: req.knot, index, siblingCount: knots.length };
}

export function SymbolContextMenuHost(): ReactElement | null {
  const symbolMenu = useStudioStore((s) => s.symbolMenu);
  const outline = useStudioStore((s) => s.outline);
  const closeSymbolMenu = useStudioStore((s) => s.closeSymbolMenu);
  const sourceBreakpoints = useStudioStore((s) => s.sourceBreakpoints);
  const breakpointToggleAtLine = useStudioStore((s) => s.breakpointToggleAtLine);
  const dispatch = useSymbolMenuActions();

  if (!symbolMenu) return null;
  const target = buildSymbolTarget(outline, symbolMenu);
  if (!target) return null;

  // Header-line breakpoints (W4/#3297, ruled 2026-08-29): the gutter click
  // on a header is play-from-here, so the header's breakpoint verb lives
  // here. Only an editor-origin request carries a line — Binder/graph
  // requests don't, and get no item.
  const line = symbolMenu.line;
  const breakpointItems =
    line === undefined
      ? undefined
      : [
          {
            label: sourceBreakpoints.some(
              (b) => b.file === symbolMenu.path && b.line === line - 1,
            )
              ? "Remove breakpoint"
              : "Set breakpoint here",
            action: () => breakpointToggleAtLine(symbolMenu.path, line - 1),
          },
        ];

  // Tag the rename action with the surface that raised the menu so the
  // dispatcher can route an editor-origin rename to the inline widget
  // (#323/#324) while graph-origin renames keep the modal prompt.
  const source = symbolMenu.source;
  const onAction = (action: ContextMenuAction): void => {
    dispatch(action.type === "renameSymbol" ? { ...action, source } : action);
  };

  return (
    <BinderContextMenu
      x={symbolMenu.x}
      y={symbolMenu.y}
      target={target}
      outline={outline}
      onAction={onAction}
      onClose={closeSymbolMenu}
      extraItems={breakpointItems}
    />
  );
}
