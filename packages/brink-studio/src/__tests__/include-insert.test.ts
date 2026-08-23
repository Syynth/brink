/**
 * Pure helpers behind the out-of-scope banner's "Add INCLUDE" action
 * (#3017): the INCLUDE path is relative to the INCLUDING file (ink's
 * resolve rule), and insertion keeps the entry's include block together.
 */
import { describe, expect, it } from "vitest";
import { insertIncludeLine, relativeIncludePath } from "@brink/studio-store";

describe("relativeIncludePath", () => {
  it("is the bare path for a root entry", () => {
    expect(relativeIncludePath("main.ink", "offcuts.ink")).toBe("offcuts.ink");
    expect(relativeIncludePath("main.ink", "scenes/harbour.ink")).toBe("scenes/harbour.ink");
  });
  it("walks down from a shared prefix for a nested entry", () => {
    expect(relativeIncludePath("story/main.ink", "story/scenes/x.ink")).toBe("scenes/x.ink");
  });
  it("walks up with ../ when the target is outside the entry's folder", () => {
    expect(relativeIncludePath("story/act2/main.ink", "story/shared/lib.ink")).toBe(
      "../shared/lib.ink",
    );
    expect(relativeIncludePath("story/main.ink", "lib.ink")).toBe("../lib.ink");
  });
});

describe("insertIncludeLine", () => {
  it("inserts at the very top of an include-less file", () => {
    expect(insertIncludeLine("Hello.\n-> END\n", "offcuts.ink")).toBe(
      "INCLUDE offcuts.ink\nHello.\n-> END\n",
    );
  });
  it("keeps the include block together — inserts after the last INCLUDE", () => {
    const src = "INCLUDE a.ink\nINCLUDE b.ink\n\nHello.\n";
    expect(insertIncludeLine(src, "c.ink")).toBe(
      "INCLUDE a.ink\nINCLUDE b.ink\nINCLUDE c.ink\n\nHello.\n",
    );
  });
  it("is a no-op when an identical INCLUDE already exists", () => {
    const src = "INCLUDE a.ink\nHello.\n";
    expect(insertIncludeLine(src, "a.ink")).toBe(src);
  });
});
