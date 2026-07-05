/**
 * Tier-1 boundary helpers published from @brink-lang/editor (#369).
 *
 * Covers: the canonical positional sortDiagnostics (file → offset →
 * errors-first), lineColAt (offset → 1-based line:col), and that the studio
 * packages' existing names (`sortDiagnostics` in @brink/studio-store,
 * `offsetToLineCol` in @brink/studio-ui) are the same functions — one
 * canonical copy, no divergent shims. CompileResult/Diagnostic re-export
 * module identity is asserted at the type level.
 */

import { describe, expect, it } from "vitest";
import {
  sortDiagnostics,
  lineColAt,
  type CompileResult as EditorCompileResult,
  type Diagnostic as EditorDiagnostic,
} from "@brink-lang/editor";
import type {
  CompileResult as WebCompileResult,
  Diagnostic as WebDiagnostic,
} from "@brink-lang/web";
import { sortDiagnostics as storeSortDiagnostics } from "@brink/studio-store";
import { offsetToLineCol } from "@brink/studio-ui";

function diag(
  file: string,
  start: number,
  severity: "Error" | "Warning",
  message = "msg",
  end = start + 1,
): EditorDiagnostic {
  return { file, start, end, severity, message };
}

// ── sortDiagnostics: canonical positional sort ──────────────────────

describe("sortDiagnostics (published boundary helper)", () => {
  it("sorts by file, then offset, then errors-first at ties", () => {
    const input = [
      diag("b.ink", 5, "Warning"),
      diag("a.ink", 10, "Warning", "w"),
      diag("a.ink", 10, "Error", "e"),
      diag("a.ink", 2, "Warning"),
      diag("b.ink", 0, "Error"),
    ];
    const sorted = sortDiagnostics(input);
    expect(sorted.map((d) => `${d.file}:${d.start}:${d.severity}`)).toEqual([
      "a.ink:2:Warning",
      "a.ink:10:Error",
      "a.ink:10:Warning",
      "b.ink:0:Error",
      "b.ink:5:Warning",
    ]);
  });

  it("is deterministic on full ties (end, then message) and non-mutating", () => {
    const input = [
      diag("a.ink", 1, "Error", "zeta", 9),
      diag("a.ink", 1, "Error", "alpha", 9),
      diag("a.ink", 1, "Error", "alpha", 3),
    ];
    const snapshot = [...input];
    const sorted = sortDiagnostics(input);
    expect(sorted.map((d) => `${d.end}:${d.message}`)).toEqual(["3:alpha", "9:alpha", "9:zeta"]);
    expect(input).toEqual(snapshot);
  });
});

// ── lineColAt: offset → 1-based line:col ────────────────────────────

describe("lineColAt (published boundary helper)", () => {
  it("computes 1-based line and column", () => {
    const text = "first\nsecond\nthird";
    expect(lineColAt(text, 0)).toEqual({ line: 1, col: 1 });
    expect(lineColAt(text, 4)).toEqual({ line: 1, col: 5 });
    expect(lineColAt(text, 6)).toEqual({ line: 2, col: 1 });
    expect(lineColAt(text, 15)).toEqual({ line: 3, col: 3 });
  });

  it("clamps offsets outside the text", () => {
    expect(lineColAt("ab\ncd", -5)).toEqual({ line: 1, col: 1 });
    expect(lineColAt("ab\ncd", 999)).toEqual({ line: 2, col: 3 });
  });
});

// ── One canonical copy: studio names delegate to the editor export ──

describe("studio packages reuse the canonical helpers", () => {
  it("@brink/studio-store sortDiagnostics IS the editor export", () => {
    expect(storeSortDiagnostics).toBe(sortDiagnostics);
  });

  it("@brink/studio-ui offsetToLineCol IS the editor lineColAt", () => {
    expect(offsetToLineCol).toBe(lineColAt);
  });
});

// ── Module identity of the type re-exports ──────────────────────────

describe("CompileResult module identity", () => {
  it("editor and web CompileResult/Diagnostic are mutually assignable", () => {
    // Type-level assertion. Note the honest limit: TypeScript's structural
    // typing means a divergent-but-structurally-identical copy would also
    // pass — this catches the shapes drifting apart, not a replaced
    // re-export. True module identity is enforced by review: the editor's
    // index.ts must re-export these types from @brink-lang/web, never
    // redeclare them.
    const result: EditorCompileResult = { ok: true };
    const asWeb: WebCompileResult = result;
    const back: EditorCompileResult = asWeb;

    const d: EditorDiagnostic = diag("a.ink", 0, "Error");
    const asWebDiag: WebDiagnostic = d;
    const backDiag: EditorDiagnostic = asWebDiag;

    expect(back.ok).toBe(true);
    expect(backDiag.file).toBe("a.ink");
  });
});
