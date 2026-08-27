/**
 * @brink/studio-shell — Continuous view (decision log 2026-08-26, "The three
 * editor views are named Code, Single File, and Continuous").
 *
 * The project as one manuscript: every file stacked in order in a single
 * scroller, with a heading between each, and you scroll straight through the
 * boundaries.
 *
 * STACKED, not concatenated. Each file keeps its own document component and so
 * its own wasm document handle, which is what keeps diagnostics, semantic
 * tokens and completion per-file and correct. The alternative — one synthetic
 * document with offset mapping back to per-file sources — would need span
 * translation across the entire IDE surface, and every feature would have to
 * know about it. This is the same shape the search result list already uses.
 *
 * §7.2: the ORDER is not the shell's business. Binder order is a studio
 * concept (the `.binder.json` sidecar), so the host passes an ordered list of
 * `DocumentRef`s and this renders them. The shell never asks what order means.
 */

import { useMemo, type ReactNode } from "react";
import { useDocumentTypes, useShell } from "./shell-context.js";
import { documentKey, type DocumentRef } from "./document.js";

/** Mount slot per file, so each section keeps a stable view across scrolls. */
function slotFor(ref: DocumentRef): string {
  return `continuous:${ref.docId}`;
}

export interface ContinuousViewProps {
  /** The files, in the order they should be read. */
  documents: readonly DocumentRef[];
  /**
   * Called when a section takes focus, so the host can make it the active
   * file — the one piece of state the views share (decision log 2026-08-26).
   */
  onActivate?: (ref: DocumentRef) => void;
  /** Highlighted as the one the caret is in. */
  activeKey?: string | null;
}

export function ContinuousView({ documents, onActivate, activeKey }: ContinuousViewProps) {
  const descriptors = useDocumentTypes();
  const types = useMemo(() => new Map(descriptors.map((d) => [d.id, d])), [descriptors]);
  const { editorGroups } = useShell();

  return (
    <section className="editor-pane shell-continuous" data-editor-view="continuous">
      <div className="shell-continuous-scroller">
        {documents.length === 0 && (
          <div className="shell-editor-empty">No files in this project</div>
        )}
        {documents.map((ref) => {
          const key = documentKey(ref);
          const descriptor = types.get(ref.typeId);
          let body: ReactNode;
          if (descriptor === undefined) {
            body = (
              <div className="shell-editor-empty" data-unknown-document-type={ref.typeId}>
                Unknown document type “{ref.typeId}”
              </div>
            );
          } else {
            const Doc = descriptor.component;
            body = <Doc doc={ref} groupId={slotFor(ref)} active={activeKey === key} />;
          }
          return (
            <article
              key={key}
              className="shell-continuous-section"
              data-continuous-file={ref.docId}
              data-active={activeKey === key || undefined}
              onFocus={() => {
                onActivate?.(ref);
                editorGroups.getState().focusGroup(editorGroups.getState().focusedGroupId);
              }}
            >
              <header className="shell-continuous-heading">
                <span className="shell-continuous-rule" />
                <span className="shell-continuous-title">{ref.title}</span>
                <span className="shell-continuous-rule" />
              </header>
              <div className="editor shell-continuous-doc">{body}</div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
