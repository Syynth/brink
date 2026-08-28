/**
 * @brink/studio-shell — Single File view (decision log 2026-08-26, "The three
 * editor views are named Code, Single File, and Continuous").
 *
 * One file at a time, with the host's companion document beside it. No tab
 * strip: navigating replaces what is on screen instead of accumulating tabs,
 * so nothing needs closing. The Binder — or search, Problems, go-to-definition
 * — is how you change file.
 *
 * The companion split is NATIVE to the view rather than a document that
 * happens to be open: it collapses and restores, but it cannot be closed into
 * an empty pane. That is the difference between "drafting a scene while
 * running it" and "having two tabs open".
 *
 * §7.2: the shell must not know what a player IS. The host names a companion
 * `DocumentRef` (`ShellProvider`'s `companionDocument`) and the shell renders
 * it through the same document-type registry as everything else — the studio
 * passes its player, another host could pass anything.
 *
 * The primary slot reads the focused group's active tab, which is what makes
 * "the active file" shared with Code view: switching views keeps the document
 * you were working on, because both views are reading the same field.
 */

import { useMemo, useState, type ReactNode } from "react";
import { Group, Panel, Separator } from "react-resizable-panels";
import {
  useDocumentTypes,
  useEditorGroups,
  useShell,
  useShellLayout,
} from "./shell-context.js";
import { documentKey, type DocumentRef } from "./document.js";
import { focusedGroup, focusedTab } from "./editor-groups.js";

/** The group id the companion's view is mounted under. */
const COMPANION_GROUP = "single-companion";

/** The group id the primary document's view is mounted under. */
const PRIMARY_GROUP = "single-primary";

export interface SingleFileViewProps {
  /**
   * The document shown beside the file. Omit it and the view is just the
   * file, full width — a host with nothing to run has no companion.
   */
  companion?: DocumentRef;
}

export function SingleFileView({ companion }: SingleFileViewProps) {
  const { editorGroups, documentIcon: DocumentIcon, layout } = useShell();
  const descriptors = useDocumentTypes();
  const types = useMemo(() => new Map(descriptors.map((d) => [d.id, d])), [descriptors]);
  const tab = useEditorGroups(focusedTab);
  const groupId = useEditorGroups((s) => focusedGroup(s).id);
  // Persisted globally (#3165) rather than component-local: this used to be
  // `useState(true)`, so every mount reopened the split — a reload, and also
  // switching to Code view and back. See `singleFileCompanionOpen` in the
  // layout store for why it is global rather than per-project.
  const companionOpen = useShellLayout((s) => s.singleFileCompanionOpen);
  const setCompanionOpen = (open: boolean): void => {
    layout.getState().setSingleFileCompanionOpen(open);
  };
  const [companionSize, setCompanionSize] = useState<number>(320);

  const primary = renderDocument(tab?.ref, PRIMARY_GROUP);
  const companionBody = companion ? renderDocument(companion, COMPANION_GROUP) : null;

  function renderDocument(ref: DocumentRef | undefined, slot: string): ReactNode {
    if (ref === undefined) return null;
    const descriptor = types.get(ref.typeId);
    if (descriptor === undefined) {
      return (
        <div className="shell-editor-empty" data-unknown-document-type={ref.typeId}>
          Unknown document type “{ref.typeId}”
        </div>
      );
    }
    const Doc = descriptor.component;
    // Keyed by document so switching file remounts rather than reusing the
    // previous file's view, and slotted by a fixed group id so the editor
    // package's per-(document, group) view cache stays stable across
    // switches instead of churning a slot per file.
    return <Doc key={documentKey(ref)} doc={ref} groupId={slot} active={true} />;
  }

  return (
    <section
      className="editor-pane shell-single-file"
      data-editor-view="single"
      onFocus={() => editorGroups.getState().focusGroup(groupId)}
    >
      <Group orientation="horizontal" id="brink-single-file" className="shell-single-file-body">
        <Panel id="single-primary" key="single-primary" minSize="240px">
          <div className="shell-single-file-primary">
            <header className="shell-single-file-head">
              {DocumentIcon && tab && <DocumentIcon doc={tab.ref} />}
              <span className="shell-single-file-name">
                {tab?.ref.title ?? "No file open"}
              </span>
              {companionBody !== null && (
                <button
                  type="button"
                  className="shell-single-file-companion-toggle"
                  aria-pressed={companionOpen}
                  onClick={() => setCompanionOpen(!companionOpen)}
                >
                  {companionOpen ? "Hide player" : "Show player"}
                </button>
              )}
            </header>
            <div className="editor shell-single-file-doc">
              {primary ?? <div className="shell-editor-empty">No file open</div>}
            </div>
          </div>
        </Panel>
        {companionBody !== null && companionOpen && (
          <Separator className="brink-resize-handle" />
        )}
        {companionBody !== null && companionOpen && (
          <Panel
            id="single-companion"
            key="single-companion"
            minSize="200px"
            defaultSize={`${companionSize}px`}
            onResize={(size) => {
              const px = Number.parseFloat(String(size));
              if (Number.isFinite(px) && px > 0) setCompanionSize(px);
            }}
          >
            <div className="shell-single-file-companion">{companionBody}</div>
          </Panel>
        )}
      </Group>
    </section>
  );
}
