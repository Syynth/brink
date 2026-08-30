/**
 * Transcript provenance conversion (W7/#3300, spec §F9).
 *
 * `TranscriptLine.source` carries UTF-8 BYTE offsets in the file as the
 * compiler consumed it; the editor speaks 0-based lines and UTF-16
 * code-unit offsets. This is the one honest conversion point: walk the
 * live file text once, tracking bytes / UTF-16 units / newlines
 * together. Registered into the store (`setSourceByteResolver`) by
 * `mount.tsx`, which can read file text; the Player consumes the
 * converted `ProvenancePoint` for its hover chip and ⌘-click reveal.
 *
 * The walk runs against the CURRENT buffer, which may have drifted from
 * the compiled text — the caller gates on `sessionDegraded` exactly as
 * the execution highlight does (suppressed, never stale); a mid-typing
 * un-recompiled buffer can still be off by the unsaved edit, which is
 * the same accepted skew the breakpoint anchors carry between compiles.
 */

import type { ProvenancePoint } from "@brink/studio-store";

/** UTF-8 byte length of one code point. */
function byteLen(cp: number): number {
  if (cp < 0x80) return 1;
  if (cp < 0x800) return 2;
  if (cp < 0x10000) return 3;
  return 4;
}

/**
 * Convert a UTF-8 byte range in `text` to editor terms. Returns `null`
 * for an out-of-range start (a stale location against shorter text —
 * never clamp into a lie). The end clamps to the text's end: a range
 * that merely OVERHANGS still points at the right place.
 */
export function provenanceFromBytes(
  text: string,
  byteStart: number,
  byteEnd: number,
): ProvenancePoint | null {
  let bytes = 0;
  let units = 0;
  let line = 0;
  let start: number | null = null;
  let startLine = 0;

  for (const ch of text) {
    if (start === null && bytes >= byteStart) {
      start = units;
      startLine = line;
    }
    if (bytes >= byteEnd && start !== null) {
      return { line: startLine, start, end: units };
    }
    const cp = ch.codePointAt(0) ?? 0;
    bytes += byteLen(cp);
    units += ch.length;
    if (ch === "\n") line += 1;
  }
  // Ranges touching the very end of the text.
  if (start === null && bytes >= byteStart) {
    start = units;
    startLine = line;
  }
  if (start === null) return null;
  return { line: startLine, start, end: units };
}
