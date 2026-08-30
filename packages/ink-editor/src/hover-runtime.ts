/**
 * Runtime-value hover augmentation (W12/#3305, spec §F15 RULED).
 *
 * While a session is live and in-sync, hovering a variable appends its
 * CURRENT runtime value to the existing hover. The editor's half is
 * deliberately dumb: extract the identifier under the cursor, ask the
 * host for a note (`getRuntimeValueNote` — the host owns the
 * globals/locals/degraded policy), and append. No new wasm surface.
 */

import type { HoverInfo } from "@brink/wasm-types";

const WORD_CHAR = /[A-Za-z0-9_]/;

/** The identifier spanning `offset` in `text`, with its bounds — or
 * `null` when the offset isn't inside a word. */
export function identifierAt(
  text: string,
  offset: number,
): { name: string; start: number; end: number } | null {
  if (offset < 0 || offset > text.length) return null;
  let start = offset;
  let end = offset;
  while (start > 0 && WORD_CHAR.test(text[start - 1])) start -= 1;
  while (end < text.length && WORD_CHAR.test(text[end])) end += 1;
  if (start === end) return null;
  const name = text.slice(start, end);
  // Pure numbers aren't identifiers — never ask the host about `42`.
  if (/^[0-9]/.test(name)) return null;
  return { name, start, end };
}

/**
 * Merge a host runtime-value note into a base hover: appended as its own
 * markdown paragraph when a base hover exists, or a value-only hover
 * anchored to the identifier's span when it doesn't.
 */
export function augmentHoverWithRuntimeValue(
  text: string,
  offset: number,
  base: HoverInfo | null,
  noteFor: (name: string) => string | null,
): HoverInfo | null {
  const word = identifierAt(text, offset);
  if (word === null) return base;
  const note = noteFor(word.name);
  if (note === null) return base;
  if (base !== null) {
    return { ...base, content: `${base.content}\n\n${note}` };
  }
  return { content: note, start: word.start, end: word.end };
}
