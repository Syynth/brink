/**
 * DocumentIcon — the shell's `documentIcon` (#3145).
 *
 * Ruled 2026-08-27, twice. First: **a file's name and its draft status
 * never appear apart** — recorded as a rule rather than a list of surfaces,
 * so this is one component the shell renders beside every document name it
 * writes (the Code view's tab, the Single File header, the Continuous
 * section heading, the takeover header) rather than four independent
 * additions. Then: **draft status is an icon variant, not a text badge** —
 * so the thing already sitting beside the name carries the status, instead
 * of a second element competing with the filename for the same row.
 *
 * It subscribes to `draftFiles` itself instead of taking status as a prop:
 * the tab strip re-renders on tab changes, not on compiles, so a prop would
 * show a stale icon until something unrelated happened to move a tab.
 *
 * Renders nothing for a document that is not a file — the Settings and
 * Story Graph takeovers name themselves, not a path, and a story-file icon
 * beside them would be a claim about a file that isn't there.
 */

import { BrinkFileDraftIcon, BrinkFileIcon } from "./icons.js";
import { INK_FILE_TYPE_ID, inkDocPath } from "./InkFileDocument.js";
import { useStudioStore } from "./StoreContext.js";

export interface DocumentIconProps {
  doc: { typeId: string; docId: string };
}

export function DocumentIcon({ doc }: DocumentIconProps) {
  const draftFiles = useStudioStore((s) => s.draftFiles);
  if (doc.typeId !== INK_FILE_TYPE_ID) return null;
  const isDraft = draftFiles.includes(inkDocPath(doc.docId));
  return (
    <span
      className="brink-doc-icon"
      data-draft={isDraft || undefined}
      title={
        isDraft
          ? "Draft — matches a `drafts` glob in brink.toml and is not reached from the entry"
          : undefined
      }
    >
      {isDraft ? <BrinkFileDraftIcon /> : <BrinkFileIcon />}
    </span>
  );
}
