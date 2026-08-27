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

import { useEffect, useMemo, useRef, type ReactNode } from "react";
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
  /**
   * The `docId` of the file the caret is in — highlighted, and scrolled to
   * when it changes.
   *
   * A docId rather than a `documentKey`, because that is what the host has:
   * the studio tracks its active document by PATH ("main.ink",
   * "main.ink::start"). Taking the composite key here instead meant the
   * comparison never matched anything, so the highlight silently never
   * appeared and navigation silently never scrolled.
   */
  activeDocId?: string | null;
  /**
   * Given a file's section, return the element navigation actually aimed at
   * — the revealed line, say — or null to settle for the section itself.
   *
   * A hook rather than a selector because finding it is editor knowledge: the
   * shell would otherwise have to hard-code a CodeMirror class name. The
   * studio passes the active-line element.
   */
  resolveRevealTarget?: (section: HTMLElement) => HTMLElement | null;
  /**
   * Changes on every navigation, even one that lands in the file already
   * open. Without it, jumping between two knots in the current file — or
   * re-picking the current file — would scroll nowhere, because the active
   * document never changed.
   */
  navSeq?: number;
}

export function ContinuousView({
  documents,
  onActivate,
  activeDocId,
  resolveRevealTarget,
  navSeq,
}: ContinuousViewProps) {
  const descriptors = useDocumentTypes();
  const types = useMemo(() => new Map(descriptors.map((d) => [d.id, d])), [descriptors]);
  const { editorGroups, documentIcon: DocumentIcon } = useShell();
  const scrollerRef = useRef<HTMLDivElement | null>(null);

  // Navigation in this view IS scrolling. Every navigation surface — the
  // Binder, search, Problems, go-to-definition — ends up setting the active
  // file, and `revealAt` moves the caret inside that file's editor. Neither
  // moves the manuscript, because the per-file editors do not scroll: they
  // size to their content and THIS is the scroller. So bringing the target
  // into view is this component's job.
  useEffect(() => {
    if (activeDocId == null || activeDocId === "") return;
    const scroller = scrollerRef.current;
    if (scroller === null) return;
    const section = scroller.querySelector<HTMLElement>(
      `[data-continuous-file="${CSS.escape(activeDocId)}"]`,
    );
    if (section === null) return;

    // Two frames: the reveal's caret move lands in the editor's own update
    // cycle, so the line to aim at does not exist yet on this one.
    const raf = requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const target = resolveRevealTarget?.(section) ?? section;
        const view = scroller.getBoundingClientRect();
        const box = target.getBoundingClientRect();
        // Already on screen? Leave it alone. Clicking into a file that is
        // right there sets the active file too, and yanking the page under
        // someone who can already see their destination is worse than doing
        // nothing.
        if (box.top >= view.top && box.bottom <= view.bottom) return;
        // Clear the sticky heading, which would otherwise cover the line.
        const heading = section.querySelector<HTMLElement>(".shell-continuous-heading");
        const clearance = heading?.getBoundingClientRect().height ?? 0;
        scroller.scrollTop += box.top - view.top - clearance;
      });
    });
    return () => cancelAnimationFrame(raf);
  }, [activeDocId, navSeq, resolveRevealTarget]);

  return (
    <section className="editor-pane shell-continuous" data-editor-view="continuous">
      <div className="shell-continuous-scroller" ref={scrollerRef}>
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
            body = <Doc doc={ref} groupId={slotFor(ref)} active={ref.docId === activeDocId} />;
          }
          return (
            <article
              key={key}
              className="shell-continuous-section"
              data-continuous-file={ref.docId}
              data-active={ref.docId === activeDocId || undefined}
              onFocus={() => {
                onActivate?.(ref);
                editorGroups.getState().focusGroup(editorGroups.getState().focusedGroupId);
              }}
            >
              <header className="shell-continuous-heading">
                <span className="shell-continuous-rule" />
                {DocumentIcon && <DocumentIcon doc={ref} />}
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
