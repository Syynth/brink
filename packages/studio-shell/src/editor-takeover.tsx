/**
 * @brink/studio-shell — a document occupying the whole editor root area
 * (decision log 2026-08-26, "The editor root area has one occupant").
 *
 * The Story Graph and Settings are whole-window activities that are not
 * files: you consult one, then go back to writing. Opening them as tabs made
 * them contents of a view, which only works for the view that HAS tabs —
 * Continuous view renders the project's files, so a Settings tab behind it
 * never appeared at all.
 *
 * A document type opts in with `takeover: true` on its descriptor; the shell
 * never learns what a graph or a setting is.
 *
 * Deliberately not persisted (see `ShellLayoutState.takeover`): relaunching
 * into Settings would be a bug, not a restore.
 *
 * NO Escape handler, deliberately. The first version added one and the
 * dismiss-net guard caught it, which turned out to be the useful question
 * rather than an obstacle: `registerDismissible` wires into a net that closes
 * EVERY transient surface at once, so Escape pressed to dismiss a completion
 * popup inside Settings would have taken Settings down with it. A full-area
 * document is not a popover and should not evaporate on a stray Escape — the
 * header's close button, and choosing any view, are the ways back.
 */

import { useMemo } from "react";
import { useDocumentTypes, useShell } from "./shell-context.js";
import { documentKey, type DocumentRef } from "./document.js";

/** The mount slot takeovers use — one at a time, so one slot. */
const TAKEOVER_GROUP = "takeover";

export function EditorTakeover({ doc }: { doc: DocumentRef }) {
  const descriptors = useDocumentTypes();
  const types = useMemo(() => new Map(descriptors.map((d) => [d.id, d])), [descriptors]);
  const { layout } = useShell();
  const descriptor = types.get(doc.typeId);

  return (
    <section className="editor-pane shell-takeover" data-takeover={doc.typeId}>
      <header className="shell-takeover-head">
        <span className="shell-takeover-title">{doc.title}</span>
        <button
          type="button"
          className="shell-takeover-close"
          aria-label={`Close ${doc.title}`}
          onClick={() => layout.getState().setTakeover(null)}
        >
          ×
        </button>
      </header>
      <div className="editor shell-takeover-body">
        {descriptor === undefined ? (
          <div className="shell-editor-empty" data-unknown-document-type={doc.typeId}>
            Unknown document type “{doc.typeId}”
          </div>
        ) : (
          <descriptor.component
            key={documentKey(doc)}
            doc={doc}
            groupId={TAKEOVER_GROUP}
            active={true}
          />
        )}
      </div>
    </section>
  );
}
