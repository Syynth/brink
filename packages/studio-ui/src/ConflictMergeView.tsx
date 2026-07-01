/**
 * ConflictMergeView — the external-conflict merge surface (issue #320,
 * Track V).
 *
 * When the host rewrites a file the studio has unsaved, divergent edits for,
 * the B1 hook keeps the buffer and the mount.tsx bridge mirrors the
 * {@link FileConflict} into the conflict slice. This component renders — for
 * the *active* document's file only — the banner ("Changed on disk while you
 * had unsaved edits", [Keep mine] / [Use disk]) plus the side-by-side 2-way
 * `@codemirror/merge` view (YOURS vs ON DISK), and routes the resolution back
 * through the store.
 *
 * The actual banner + MergeView are owned by the framework-agnostic
 * {@link ConflictView} (in @brink-lang/editor): this component only mounts it
 * into a container for its lifetime and tears it down on unmount / conflict
 * change — `ConflictView.destroy()` removes every listener + DOM node and
 * destroys the MergeView (CM6 teardown contract; leaks are bugs).
 *
 * It overlays the editor area rather than living in a tab: a conflict is a
 * modal "you must reconcile this" moment, and the conflicted file's normal
 * editor sits underneath unchanged (the kept buffer).
 */

import { useEffect, useRef } from "react";
import { ConflictView, type FileConflict } from "@brink-lang/editor";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";

/** The file path for a docKey ("main.ink" or "main.ink::start"). */
function pathOfDocKey(docKey: string): string {
  const sep = docKey.indexOf("::");
  return sep < 0 ? docKey : docKey.slice(0, sep);
}

export function ConflictMergeView() {
  const storeApi = useStudioStoreApi();
  const activeDocKey = useStudioStore((s) => s.activeDocKey);
  const conflicts = useStudioStore((s) => s.conflicts);

  const activePath = activeDocKey === "" ? null : pathOfDocKey(activeDocKey);
  const conflict: FileConflict | null =
    activePath !== null ? (conflicts[activePath] ?? null) : null;

  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<ConflictView | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !conflict) return;

    const { resolveUseDisk, resolveKeepMine, resolveMerge } = storeApi.getState();
    const path = conflict.path;

    const view = new ConflictView(host, conflict, {
      onUseDisk: () => resolveUseDisk(path),
      onKeepMine: () => resolveKeepMine(path),
      onMerge: (merged) => resolveMerge(path, merged),
    });
    viewRef.current = view;

    return () => {
      // CM6 teardown: destroy the MergeView + listeners + DOM.
      view.destroy();
      viewRef.current = null;
    };
    // Re-mount when the active conflict identity changes (path or either side
    // of the diff). The merge view caches the texts, so a new conflict needs a
    // fresh instance.
  }, [storeApi, conflict?.path, conflict?.disk, conflict?.buffer, conflict]);

  if (!conflict) return null;

  return (
    <div className="brink-conflict-overlay" role="dialog" aria-modal="true" aria-label="Resolve file conflict">
      <div className="brink-conflict-panel" ref={hostRef} />
    </div>
  );
}
