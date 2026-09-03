/**
 * Byte range → editor terms (W7/#3300): the end LINE rides along so a
 * transcript line built from several source lines (glue, a cue + aside +
 * dialogue) highlights every line it came from (feedback 2026-09-02).
 */
import { describe, expect, it } from "vitest";
import { provenanceFromBytes } from "../transcript-provenance";

describe("provenanceFromBytes", () => {
  const text = "GRISWOLD\n(not getting up)\nBuying or dying?\nNext.\n";

  it("a single-line range starts and ends on the same line", () => {
    expect(provenanceFromBytes(text, 0, 8)).toEqual({ line: 0, endLine: 0, start: 0, end: 8 });
  });

  it("a range spanning three source lines reports the last line it touches", () => {
    const end = text.indexOf("Next.") - 1; // through "dying?", before its newline
    expect(provenanceFromBytes(text, 0, end)).toEqual({ line: 0, endLine: 2, start: 0, end });
  });

  it("a range whose end sits on a newline does not spill onto the next line", () => {
    const end = text.indexOf("Next."); // the newline after "dying?" is inside
    expect(provenanceFromBytes(text, 0, end)).toMatchObject({ line: 0, endLine: 2 });
  });

  it("multi-byte text converts bytes to UTF-16 units and keeps the lines", () => {
    const t = "é\nab\n";
    expect(provenanceFromBytes(t, 3, 5)).toEqual({ line: 1, endLine: 1, start: 2, end: 4 });
  });

  it("an out-of-range start is null; an overhanging end clamps", () => {
    expect(provenanceFromBytes("ab", 5, 6)).toBeNull();
    expect(provenanceFromBytes("ab\ncd", 3, 99)).toEqual({ line: 1, endLine: 1, start: 3, end: 5 });
  });
});
