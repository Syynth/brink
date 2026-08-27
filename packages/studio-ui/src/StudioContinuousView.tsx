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

import { useCallback, useMemo } from "react";
import { ContinuousView, type DocumentRef } from "@brink/studio-shell";

import { useStudioStore } from "./StoreContext.js";
import { binderOrderedFiles } from "./Binder.js";
import { inkFileRef } from "./InkFileDocument.js";

export function StudioContinuousView() {
  const outline = useStudioStore((s) => s.outline);
  const binderOrder = useStudioStore((s) => s.binderOrder);
  const entryFile = useStudioStore((s) => s.entryFile);
  const openTarget = useStudioStore((s) => s.openTarget);
  const activeDocKey = useStudioStore((s) => s.activeDocKey);
  const navSeq = useStudioStore((s) => s.navSeq);

  const documents = useMemo<DocumentRef[]>(
    () =>
      binderOrderedFiles(outline, binderOrder, entryFile ?? null).map((path) =>
        inkFileRef({ kind: "file", path }),
      ),
    [outline, binderOrder, entryFile],
  );

  // Where navigation actually aimed inside the file. `revealAt` moves the
  // caret, and CodeMirror marks the line it landed on — so the active line is
  // the target, when there is one. Kept on this side of §7.2: the shell would
  // otherwise have to know a CodeMirror class name to scroll correctly.
  const resolveRevealTarget = useCallback(
    (section: HTMLElement) => section.querySelector<HTMLElement>(".cm-activeLine"),
    [],
  );

  return (
    <ContinuousView
      documents={documents}
      // The store's active doc key is a PATH, and may name a symbol
      // ("main.ink::start"); the sections are files, so take the file half.
      activeDocId={activeDocKey.split("::")[0] || null}
      resolveRevealTarget={resolveRevealTarget}
      navSeq={navSeq}
      // Focusing a section makes that file the active one — the state the
      // views share, so scrolling to a scene here and switching to Single
      // File lands on the scene you were reading.
      onActivate={(ref: DocumentRef) => {
        if (ref.docId === activeDocKey.split("::")[0]) return;
        openTarget({ kind: "file", path: ref.docId }, false);
      }}
    />
  );
}
