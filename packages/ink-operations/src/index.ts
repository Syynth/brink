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

/**
 * A convertible-kind's source shape, reduced to what `extractLineContent`
 * needs: a compiled pattern and which named group is the editable content
 * (mirrors `PatternShape` in `@brink/wasm-types`, e.g. produced from a
 * resolved dialect's `templateFor`/pattern for a declared kind). Passed by a
 * dialect-aware caller (`transitions.ts`'s `executeDialectRow` `convert`
 * action) so extraction follows the resolved dialect's own shape instead of
 * the hardcoded at-cue regexes below (#395).
 */
export interface ConvertibleShape {
  /** Portable-regex pattern, anchored against the trimmed line. */
  pattern: string;
  /** Which named group is the editable content. */
  contentGroup?: string | null;
}

/** The at-cue preset's built-in wrapping shapes — the pre-#395 hardcoded
 *  behavior, kept as the fallback so a call with no `shapes` argument (or a
 *  shape list that doesn't match) stays byte-identical to before. */
const DEFAULT_CONVERTIBLE_SHAPES: ConvertibleShape[] = [
  { pattern: "^@(?<content>[^:]*):<>$", contentGroup: "content" }, // Character: @Name:<> → Name
  { pattern: "^\\((?<content>.*)\\)<>$", contentGroup: "content" }, // Parenthetical: (text)<> → text
];

/**
 * Extract the "meat" from a line, stripping any wrapping or prefix sigils.
 *
 * `shapes` (#395), when given, is tried first, in order — each shape's
 * `pattern` is matched against the trimmed line and, on a match, the named
 * `contentGroup` group's text is returned. This lets a custom dialect's
 * wrapping kinds (not just the built-in `@name:<>`/`(text)<>`) extract
 * correctly. Falls through to the built-in at-cue shapes, then prefix-sigil
 * stripping via `getLineSigilRange`, when `shapes` is omitted or none match —
 * byte-identical to pre-#395 behavior for the default preset.
 */
export function extractLineContent(text: string, shapes?: readonly ConvertibleShape[]): string {
  const trimmed = text.trimStart();
  for (const shape of [...(shapes ?? []), ...DEFAULT_CONVERTIBLE_SHAPES]) {
    if (!shape.contentGroup) continue;
    let re: RegExp;
    try {
      re = new RegExp(shape.pattern);
    } catch {
      continue;
    }
    const m = trimmed.match(re);
    const content = m?.groups?.[shape.contentGroup];
    if (content !== undefined) return content;
  }
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
