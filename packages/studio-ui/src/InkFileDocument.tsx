/**
 * InkFileDocument — the "ink-file" document type (spec §7.8).
 *
 * One CM6 EditorView per (document, group) instance, mounted by the shell's
 * EditorArea through the document-type registry. All the machinery — wasm
 * document handles, state caching, cross-view mirroring, focus tracking —
 * lives in DocumentSessions (@brink/ink-editor); this component only binds a
 * view slot to a DOM node for its mount lifetime.
 *
 * Document ids reuse the old tab-id scheme: `"main.ink"` for files,
 * `"main.ink::intro"` for symbol (fragment) documents.
 */

import { useEffect, useRef } from "react";
import type { DocumentRef, DocumentViewProps } from "@brink/studio-shell";
import { docKeyFor, docTitleFor, type TabTarget } from "@brink/studio-store";
import { useStudioStoreApi } from "./StoreContext.js";

export const INK_FILE_TYPE_ID = "ink-file";

/** Build the shell DocumentRef for an ink file/symbol target. */
export function inkFileRef(target: TabTarget): DocumentRef {
  return {
    typeId: INK_FILE_TYPE_ID,
    docId: docKeyFor(target),
    title: docTitleFor(target),
  };
}

export function InkFileDocument({ doc, groupId }: DocumentViewProps) {
  const storeApi = useStudioStoreApi();
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const documents = storeApi.getState()._documents;
    const container = containerRef.current;
    if (!documents || !container) return;
    return documents.mountView(doc.docId, groupId, container);
  }, [storeApi, doc.docId, groupId]);

  return (
    <div
      ref={containerRef}
      className="brink-ink-document"
      style={{ height: "100%", width: "100%" }}
    />
  );
}
