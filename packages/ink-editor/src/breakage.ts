/**
 * Breakage-report pure logic (#316/#323/#324) — the safe/unsafe verdict and the
 * affected-reference list derived from a {@link StructuralResult}. Shared by the
 * inline rename (rename.ts) and the extract prompt (inline-name-input.ts); both
 * render the same "⚠ breaks N" badge + inline report from these helpers.
 *
 * Kept framework-free (no CodeMirror) so it is directly unit-testable.
 */

import type { StructuralResult } from "@brink/wasm-types";

/** A single affected reference in the inline breakage report. */
export interface BreakageEntry {
  /** Project-relative file path of the affected reference. */
  file: string;
  /** 1-based line, when known (introduced diagnostics carry it). */
  line?: number;
  /** 1-based column, when known. */
  col?: number;
  /** Human-readable detail (the diagnostic message, or a generic note). */
  message: string;
}

/** Whether `result` is safe (no introduced diagnostics). Named for its rename
 *  origin; applies to every structural op. */
export function isSafeRename(result: StructuralResult): boolean {
  return result.safe && result.introduced_diagnostics.length === 0;
}

/** The breakage count for the "⚠ breaks N" badge — the number of introduced
 *  diagnostics. 0 when safe (the badge is then hidden). */
export function breakageCount(result: StructuralResult): number {
  return result.introduced_diagnostics.length;
}

/**
 * The affected-reference list for the inline report: one entry per introduced
 * diagnostic (`file:line` + message), falling back to the cross-file edits
 * (file-level, no line) when a result reports edits but no diagnostics.
 * Deterministic — diagnostics keep their result order; cross-file fallbacks are
 * sorted by path.
 */
export function breakageEntries(result: StructuralResult): BreakageEntry[] {
  if (result.introduced_diagnostics.length > 0) {
    return result.introduced_diagnostics.map((d) => ({
      file: d.path,
      line: d.line,
      col: d.col,
      message: d.message,
    }));
  }
  return [...result.cross_file_edits]
    .map((e) => e.path)
    .sort()
    .map((path) => ({ file: path, message: "reference updated" }));
}
