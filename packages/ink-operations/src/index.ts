/**
 * @brink/ink-operations — Pure functions for document structural edits.
 *
 * No CM6, no React, no wasm runtime dependencies.
 * Takes strings and outlines, returns edit operations.
 */

// ── Convertible types ────────────────────────────────────────────

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

// ── Content extraction ───────────────────────────────────────────

/** Extract the "meat" from a line, stripping any wrapping or prefix sigils. */
export function extractLineContent(text: string): string {
  const trimmed = text.trimStart();
  // Character: @Name:<> → Name
  const charMatch = trimmed.match(/^@([^:]*):<>$/);
  if (charMatch) return charMatch[1];
  // Parenthetical: (text)<> → text
  const parenMatch = trimmed.match(/^\((.*)\)<>$/);
  if (parenMatch) return parenMatch[1];
  // Prefix sigils: strip via getLineSigilRange
  const { end } = getLineSigilRange(text);
  return text.slice(end);
}

// ── Sigil range detection ────────────────────────────────────────

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
