/**
 * InkFileDocument — the "ink-file" document type (spec §7.8).
 *
 * One CM6 EditorView per (document, group) instance, mounted by the shell's
 * EditorArea through the document-type registry. All the machinery — wasm
 * document handles, state caching, cross-view mirroring, focus tracking —
 * lives in DocumentSessions (@brink-lang/editor); this component only binds a
 * view slot to a DOM node for its mount lifetime.
 *
 * Document ids reuse the old tab-id scheme: `"main.ink"` for files,
 * `"main.ink::intro"` for symbol (fragment) documents.
 *
 * The out-of-scope banner (#3017 — compare
 * `docs/design/project-open-flow/ScopeBanner.dc.html`) renders above the
 * editor when the file is a source file OUTSIDE the latest compile's
 * closure: absent diagnostics on such a file look identical to clean
 * diagnostics, so the editor says so. Informational, not alarming — the
 * statement is true, not an error. "Add INCLUDE to <entry>" (offered for
 * the ink flow: an `.ink` file under an `.ink` entry) dispatches the
 * compile slice's `includeInEntry`, which inserts the INCLUDE, refreshes
 * the entry's view, and recompiles — clearing the banner by making the
 * statement false.
 */

import { useEffect, useMemo, useRef } from "react";
import type { DocumentRef, DocumentViewProps } from "@brink/studio-shell";
import { docKeyFor, docTitleFor, type TabTarget } from "@brink/studio-store";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { ConfigFormPanel, isConfigPath } from "./ConfigFormPanel.js";

export const INK_FILE_TYPE_ID = "ink-file";

/** Build the shell DocumentRef for an ink file/symbol target. */
export function inkFileRef(target: TabTarget): DocumentRef {
  return {
    typeId: INK_FILE_TYPE_ID,
    docId: docKeyFor(target),
    title: docTitleFor(target),
  };
}

/** The file path behind a doc id ("path" or "path::symbol"). */
export function inkDocPath(docId: string): string {
  const sep = docId.indexOf("::");
  return sep < 0 ? docId : docId.slice(0, sep);
}

/**
 * Whether `path` is a source file outside the latest compile closure
 * (#3017). False before the first compile (`closure` empty — nothing to
 * contradict yet), for non-source files (`brink.toml`, …), and for mounted
 * stdlib files (they get the Library treatment, #2306, not this banner).
 */
export function isOutOfScope(
  path: string,
  closure: string[],
  outline: { path: string; mounted?: boolean }[],
): boolean {
  if (closure.length === 0) return false;
  if (!path.endsWith(".ink") && !path.endsWith(".brink")) return false;
  if (closure.includes(path)) return false;
  return outline.find((f) => f.path === path)?.mounted !== true;
}

const INFO_ICON = (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
  >
    <circle cx="12" cy="12" r="10" />
    <line x1="12" y1="16" x2="12" y2="12" />
    <line x1="12" y1="8" x2="12.01" y2="8" />
  </svg>
);

export function InkFileDocument({ doc, groupId }: DocumentViewProps) {
  const storeApi = useStudioStoreApi();
  const containerRef = useRef<HTMLDivElement>(null);
  const path = inkDocPath(doc.docId);
  const closure = useStudioStore((s) => s.closureFiles);
  const outline = useStudioStore((s) => s.outline);
  const includeInEntry = useStudioStore((s) => s.includeInEntry);

  const outOfScope = useMemo(
    () => isOutOfScope(path, closure, outline),
    [path, closure, outline],
  );
  // The entry is re-read per compile (closure identity changes each
  // compile), which is exactly as often as it can change.
  const entryFile = useMemo(
    () => storeApi.getState()._project?.getEntryFile() ?? null,
    // eslint-disable-next-line react-hooks/exhaustive-deps -- closure is the refresh signal, see above
    [storeApi, closure],
  );
  const entryBase = entryFile === null ? null : (entryFile.split("/").at(-1) ?? entryFile);
  const canInclude =
    outOfScope &&
    path.endsWith(".ink") &&
    entryFile !== null &&
    entryFile.endsWith(".ink") &&
    entryFile !== path;

  useEffect(() => {
    const documents = storeApi.getState()._documents;
    const container = containerRef.current;
    if (!documents || !container) return;
    return documents.mountView(doc.docId, groupId, container);
  }, [storeApi, doc.docId, groupId]);

  return (
    <div className="brink-ink-document-frame">
      {isConfigPath(path) && <ConfigFormPanel path={path} />}
      {outOfScope && (
        <div className="brink-scope-banner" role="note">
          <span className="scope-banner-icon">{INFO_ICON}</span>
          <span className="scope-banner-msg">
            Not included in the project — nothing <code>INCLUDE</code>s this file, so it is not
            analyzed and will not appear in the story.
          </span>
          {canInclude && (
            <button
              className="scope-banner-include"
              onClick={() => includeInEntry(path)}
              title={`Insert INCLUDE for this file into ${entryBase ?? "the entry file"}`}
            >
              Add INCLUDE to {entryBase}
            </button>
          )}
        </div>
      )}
      <div ref={containerRef} className="brink-ink-document" />
    </div>
  );
}
