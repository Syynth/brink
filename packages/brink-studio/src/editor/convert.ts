/**
 * Shared line-type conversion logic.
 *
 * Used by the status bar dropdown, the inline element picker (Alt+Enter),
 * and any future conversion entry points.
 */

import type { EditorView } from "@codemirror/view";
import { sigilBypass } from "./screenplay.js";

// ── Convertible types ─────────────────────────────────────────────

export const CONVERTIBLE_TYPES: { label: string; sigil: string; key: string }[] = [
  { label: "Narrative", sigil: "", key: "n" },
  { label: "Choice (*)", sigil: "* ", key: "c" },
  { label: "Choice (+)", sigil: "+ ", key: "s" },
  { label: "Gather", sigil: "- ", key: "g" },
  { label: "Divert", sigil: "-> ", key: "d" },
  { label: "Logic", sigil: "~ ", key: "l" },
  { label: "Comment", sigil: "// ", key: "/" },
  { label: "Tag", sigil: "# ", key: "t" },
  { label: "Knot Header", sigil: "=== ", key: "k" },
  { label: "Stitch Header", sigil: "= ", key: "h" },
  { label: "Character", sigil: "@:<>", key: "@" },
  { label: "Parenthetical", sigil: "()<>", key: "p" },
];

// ── Content extraction ────────────────────────────────────────────

/** Extract the "meat" from a line, stripping any wrapping sigils. */
export function extractLineContent(text: string): string {
  const trimmed = text.trimStart();
  const charMatch = trimmed.match(/^@([^:]*):<>$/);
  if (charMatch) return charMatch[1];
  const parenMatch = trimmed.match(/^\((.*)\)<>$/);
  if (parenMatch) return parenMatch[1];
  return trimmed;
}

// ── Sigil range detection ─────────────────────────────────────────

export function getLineSigilRange(text: string): { start: number; end: number } {
  const trimmed = text.trimStart();
  const ws = text.length - trimmed.length;

  if (/^@[^:]*:<>$/.test(trimmed)) {
    return { start: ws, end: ws + trimmed.length };
  }
  if (/^\(.*\)<>$/.test(trimmed)) {
    return { start: ws, end: ws + trimmed.length };
  }

  const patterns = [
    /^={3,}\s*/,
    /^={2}\s+\w[^=]*={2,}\s*/,
    /^=\s+/,
    /^([*+]\s*)+/,
    /^(-\s*)+(?!>)/,
    /^->\s*/,
    /^~\s*/,
    /^\/\/\s*/,
    /^\/\*\s*/,
    /^#\s*/,
    /^(VAR|CONST|LIST)\s+/,
    /^INCLUDE\s+/,
    /^EXTERNAL\s+/,
  ];

  for (const p of patterns) {
    const m = trimmed.match(p);
    if (m) return { start: ws, end: ws + m[0].length };
  }

  return { start: ws, end: ws };
}

// ── Line conversion ───────────────────────────────────────────────

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
