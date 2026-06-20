/**
 * React hook wrapping {@link dispatchSymbolAction} for the editor and Story
 * Graph context menus (and reusable anywhere). Fire-and-forget — the Binder
 * uses `dispatchSymbolAction` directly where it needs to await.
 */

import { useCallback } from "react";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import type { ContextMenuAction } from "./BinderContextMenu.js";
import { dispatchSymbolAction } from "./symbolMenuActions.js";

export function useSymbolMenuActions(): (action: ContextMenuAction) => void {
  const storeApi = useStudioStoreApi();
  const applyMoveResult = useStudioStore((s) => s.applyMoveResult);

  return useCallback(
    (action: ContextMenuAction) => {
      void dispatchSymbolAction(storeApi.getState(), applyMoveResult, action);
    },
    [storeApi, applyMoveResult],
  );
}
