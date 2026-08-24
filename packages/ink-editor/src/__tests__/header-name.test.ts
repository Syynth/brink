/**
 * Header-name parsing for play-from-here / the symbol context menu (#3054
 * review): the `function` keyword is not part of the ink path — before the
 * fix, right-clicking `=== function carrying(item) ===` resolved the path
 * "function carrying" and the menu request silently failed ("works on knots
 * but not functions").
 */
import { describe, expect, it } from "vitest";
import { headerName } from "../play-from-here.js";

describe("headerName", () => {
  it("plain knot header", () => {
    expect(headerName("=== intro ===")).toBe("intro");
  });
  it("function header strips the keyword", () => {
    expect(headerName("=== function carrying(item) ===")).toBe("carrying");
  });
  it("function header without params", () => {
    expect(headerName("== function party_size() ==")).toBe("party_size");
  });
  it("stitch header", () => {
    expect(headerName("= letter")).toBe("letter");
  });
  it("knot with params keeps only the name", () => {
    expect(headerName("=== greet(name) ===")).toBe("greet");
  });
});
