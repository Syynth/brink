/**
 * DraftMark — the shell's `documentMark` (#3145).
 *
 * Ruled 2026-08-27: **a file's name and its draft status never appear
 * apart.** That was recorded as a rule rather than a list of surfaces on
 * purpose, so this is one component the shell renders beside every document
 * name it writes — the Code view's tab, the Single File header, the
 * Continuous section heading, the takeover header — rather than four
 * independent additions. A naming surface added later inherits the rule by
 * using the same seam.
 *
 * It subscribes to `draftFiles` itself instead of taking status as a prop:
 * the tab strip re-renders on tab changes, not on compiles, so a prop would
 * show a stale mark until something unrelated happened to move a tab.
 *
 * Renders nothing for a non-draft, and nothing for a document that is not a
 * file (the Settings/Graph takeovers name themselves, not a path).
 */

import { INK_FILE_TYPE_ID, inkDocPath } from "./InkFileDocument.js";
import { useStudioStore } from "./StoreContext.js";

export interface DraftMarkProps {
  doc: { typeId: string; docId: string };
}

export function DraftMark({ doc }: DraftMarkProps) {
  const draftFiles = useStudioStore((s) => s.draftFiles);
  if (doc.typeId !== INK_FILE_TYPE_ID) return null;
  if (!draftFiles.includes(inkDocPath(doc.docId))) return null;
  return (
    <span
      className="brink-draft-mark"
      title="Draft — matches a `drafts` glob in brink.toml and is not reached from the entry"
    >
      Draft
    </span>
  );
}
