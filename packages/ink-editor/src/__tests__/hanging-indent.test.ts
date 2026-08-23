/**
 * Hanging indent for soft-wrapped lines (the 2026-08-23 literal-whitespace
 * ruling's companion): column math for the `--line-indent` carrier. The
 * DOM half (the custom property landing on lines, and nothing else) is
 * pinned by the studio's structural-decoration-attrs audit.
 */
import { describe, expect, it } from "vitest";
import { indentColumns } from "../hanging-indent.js";

describe("indentColumns", () => {
  it("counts spaces one column each", () => {
    expect(indentColumns("    text", 4)).toBe(4);
    expect(indentColumns("text", 4)).toBe(0);
    expect(indentColumns("", 4)).toBe(0);
  });
  it("advances tabs to the next tab stop, matching the renderer", () => {
    expect(indentColumns("\ttext", 4)).toBe(4);
    expect(indentColumns("\t\ttext", 4)).toBe(8);
    // A space then a tab lands on the SAME stop as a bare tab.
    expect(indentColumns(" \ttext", 4)).toBe(4);
    expect(indentColumns("  \t text", 4)).toBe(5);
  });
  it("stops at the first non-whitespace character", () => {
    expect(indentColumns("  * choice  with spaces", 4)).toBe(2);
  });
});
