/**
 * The studio's Continuous view content (decision log 2026-08-26).
 *
 * The shell renders the stack; this decides WHAT is in it and in what order,
 * because both are studio concepts. Binder order comes from the
 * `.binder.json` sidecar via `binderOrderedFiles`, which is built on the same
 * `buildFlatRows` the Binder tree uses — so the manuscript reads in exactly
 * the order the Binder shows, by construction rather than by agreement.
 *
 * This lives on the studio side of §7.2 precisely because it touches the
 * store: `ShellProvider` takes it as an element and renders it inside the
 * tree, where the store's context is available.
 */

import { useMemo } from "react";
import { ContinuousView, documentKey, type DocumentRef } from "@brink/studio-shell";

import { useStudioStore } from "./StoreContext.js";
import { binderOrderedFiles } from "./Binder.js";
import { inkFileRef } from "./InkFileDocument.js";

export function StudioContinuousView() {
  const outline = useStudioStore((s) => s.outline);
  const binderOrder = useStudioStore((s) => s.binderOrder);
  const entryFile = useStudioStore((s) => s.entryFile);
  const openTarget = useStudioStore((s) => s.openTarget);
  const activeDocKey = useStudioStore((s) => s.activeDocKey);

  const documents = useMemo<DocumentRef[]>(
    () =>
      binderOrderedFiles(outline, binderOrder, entryFile ?? null).map((path) =>
        inkFileRef({ kind: "file", path }),
      ),
    [outline, binderOrder, entryFile],
  );

  return (
    <ContinuousView
      documents={documents}
      activeKey={activeDocKey}
      // Focusing a section makes that file the active one — the state the
      // views share, so scrolling to a scene here and switching to Single
      // File lands on the scene you were reading.
      onActivate={(ref: DocumentRef) => {
        if (documentKey(ref) === activeDocKey) return;
        openTarget({ kind: "file", path: ref.docId }, false);
      }}
    />
  );
}
