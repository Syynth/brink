/**
 * Shared line-type conversion logic.
 *
 * Pure functions re-exported from @brink/ink-operations.
 * The CM6-dependent convertLineToType dispatch stays here.
 */

import type { EditorView } from "@codemirror/view";
import { sigilBypass } from "./screenplay.js";
import { extractLineContent } from "@brink/ink-operations";

// Re-export pure functions from @brink/ink-operations
export { CONVERTIBLE_TYPES, extractLineContent, getLineSigilRange } from "@brink/ink-operations";

// ── Line conversion (CM6 dispatch) ──────────────────────────────

export function convertLineToType(view: EditorView, sigil: string): void {
  const pos = view.state.selection.main.head;
  const line = view.state.doc.lineAt(pos);
  const content = extractLineContent(line.text);

  // Wrapping sigils: character and parenthetical wrap content
  if (sigil === "@:<>") {
    const newText = "@" + content + ":<>";
    view.dispatch({
      changes: { from: line.from, to: line.to, insert: newText },
      selection: { anchor: line.from + 1 + content.length },
      annotations: sigilBypass.of(true),
    });
    view.focus();
    return;
  }
  if (sigil === "()<>") {
    const newText = "(" + content + ")<>";
    view.dispatch({
      changes: { from: line.from, to: line.to, insert: newText },
      selection: { anchor: line.from + 1 + content.length },
      annotations: sigilBypass.of(true),
    });
    view.focus();
    return;
  }

  // Prefix sigils: replace entire line with sigil + extracted content
  view.dispatch({
    changes: { from: line.from, to: line.to, insert: sigil + content },
    selection: { anchor: line.from + sigil.length + content.length },
    annotations: sigilBypass.of(true),
  });
  view.focus();
}
