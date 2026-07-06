/**
 * Tier-1 boundary helpers (#369) — the small pure functions every host needs
 * at the editor ↔ host seam, published so embedders don't carry shim copies.
 *
 * These are the canonical implementations; the studio packages re-export them.
 */

import type { Diagnostic } from "@brink-lang/web";

/**
 * Canonical positional diagnostic ordering (deterministic): file path, then
 * start offset, then errors before warnings, then end offset and message as
 * tiebreakers. Non-mutating — returns a new array.
 *
 * Presentation ORDER is a host choice layered on top of this sort: hosts may
 * re-group the canonically sorted list (e.g. severity-first, per-file
 * sections) for display. This helper is the shared positional baseline, not
 * a rendering policy.
 */
export function sortDiagnostics(diagnostics: readonly Diagnostic[]): Diagnostic[] {
  return [...diagnostics].sort((a, b) => {
    if (a.file !== b.file) return a.file < b.file ? -1 : 1;
    if (a.start !== b.start) return a.start - b.start;
    if (a.severity !== b.severity) return a.severity === "Error" ? -1 : 1;
    if (a.end !== b.end) return a.end - b.end;
    if (a.message !== b.message) return a.message < b.message ? -1 : 1;
    return 0;
  });
}

/**
 * 1-based line:col for a UTF-16 offset into `text` (clamped to the text).
 * Matches the offset space of `Diagnostic.start`/`end` and `editor.reveal`
 * source spans.
 */
export function lineColAt(text: string, offset: number): { line: number; col: number } {
  const clamped = Math.max(0, Math.min(offset, text.length));
  let line = 1;
  let lineStart = 0;
  for (let i = 0; i < clamped; i++) {
    if (text.charCodeAt(i) === 10 /* \n */) {
      line++;
      lineStart = i + 1;
    }
  }
  return { line, col: clamped - lineStart + 1 };
}
