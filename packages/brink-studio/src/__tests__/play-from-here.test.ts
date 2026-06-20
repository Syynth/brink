import { describe, expect, it } from "vitest";
import { ElementType, headerName, qualifiedInkPath } from "@brink/ink-editor";

describe("headerName", () => {
  it("strips knot/stitch sigils and params", () => {
    expect(headerName("=== intro ===")).toBe("intro");
    expect(headerName("== intro")).toBe("intro");
    expect(headerName("= evidence")).toBe("evidence");
    expect(headerName("  =  alibi  ")).toBe("alibi");
    expect(headerName("=== call(action, present) ===")).toBe("call");
  });

  it("returns null for an empty header", () => {
    expect(headerName("===  ===")).toBeNull();
    expect(headerName("")).toBeNull();
  });
});

describe("qualifiedInkPath", () => {
  // 1: === intro ===     KnotHeader
  // 2: Hi.               Narrative
  // 3: = evidence        StitchHeader
  // 4: A clue.           Narrative
  // 5: === shop(x) ===   KnotHeader
  const texts = ["=== intro ===", "Hi.", "= evidence", "A clue.", "=== shop(x) ==="];
  const types = [
    ElementType.KnotHeader,
    ElementType.NarrativeText,
    ElementType.StitchHeader,
    ElementType.NarrativeText,
    ElementType.KnotHeader,
  ];

  it("resolves a knot header to its bare name", () => {
    expect(qualifiedInkPath(texts, types, 1)).toBe("intro");
    expect(qualifiedInkPath(texts, types, 5)).toBe("shop");
  });

  it("resolves a stitch to knot.stitch via the enclosing knot", () => {
    expect(qualifiedInkPath(texts, types, 3)).toBe("intro.evidence");
  });

  it("returns null for non-header lines", () => {
    expect(qualifiedInkPath(texts, types, 2)).toBeNull();
    expect(qualifiedInkPath(texts, types, 4)).toBeNull();
  });

  it("falls back to the bare stitch name with no enclosing knot", () => {
    expect(qualifiedInkPath(["= orphan"], [ElementType.StitchHeader], 1)).toBe("orphan");
  });
});
