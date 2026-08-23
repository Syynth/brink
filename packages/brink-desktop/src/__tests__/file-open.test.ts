import { describe, expect, it } from "vitest";
import { parentDir, relativeToRoot, resolveFileOpenAction } from "../file-open.js";

describe("parentDir", () => {
  it("returns everything before the last slash", () => {
    expect(parentDir("/Users/ben/story/scenes/intro.brink")).toBe("/Users/ben/story/scenes");
  });

  it("falls back to the filesystem root for a bare top-level path", () => {
    expect(parentDir("/story.brink")).toBe("/");
  });
});

describe("relativeToRoot", () => {
  it("strips the root prefix for a file inside it", () => {
    expect(relativeToRoot("/Users/ben/story", "/Users/ben/story/scenes/intro.brink")).toBe(
      "scenes/intro.brink",
    );
  });

  it("is tolerant of a root with a trailing slash", () => {
    expect(relativeToRoot("/Users/ben/story/", "/Users/ben/story/main.ink")).toBe("main.ink");
  });

  it("returns null for a path outside the root", () => {
    expect(relativeToRoot("/Users/ben/story", "/Users/ben/other/main.ink")).toBeNull();
  });

  it("returns null for a path that merely shares the root as a string prefix", () => {
    // "/Users/ben/story-extra/main.ink" starts with "/Users/ben/story" but
    // is not inside it — the "/" join in relativeToRoot must reject this.
    expect(relativeToRoot("/Users/ben/story", "/Users/ben/story-extra/main.ink")).toBeNull();
  });
});

describe("resolveFileOpenAction", () => {
  it("focuses in place when a project is open and the file is inside it", () => {
    expect(
      resolveFileOpenAction("/Users/ben/story/scenes/intro.brink", "/Users/ben/story"),
    ).toEqual({ kind: "focus", rel: "scenes/intro.brink" });
  });

  it("opens the containing folder as the project when none is open", () => {
    expect(resolveFileOpenAction("/Users/ben/other/main.ink", null)).toEqual({
      kind: "open",
      root: "/Users/ben/other",
      rel: "main.ink",
      entryIsExplicit: true,
    });
  });

  it("keeps the legacy folder door (no explicit entry) for a .brink open", () => {
    // Native file-anchoring is deferred (#3021): a `.brink` double-click
    // opens the surrounding folder the pre-#3021 way, where a brink.toml
    // may still supersede the entry.
    expect(resolveFileOpenAction("/Users/ben/other/mod.brink", null)).toEqual({
      kind: "open",
      root: "/Users/ben/other",
      rel: "mod.brink",
      entryIsExplicit: false,
    });
  });

  it("opens the new containing folder when the file is outside the open project", () => {
    // The caller runs this through the existing `openProject` seam, which
    // already close-saves the previous project before mounting the new
    // one — this module only decides WHAT to do, not how to tear down.
    expect(
      resolveFileOpenAction("/Users/ben/other/main.ink", "/Users/ben/story"),
    ).toEqual({
      kind: "open",
      root: "/Users/ben/other",
      rel: "main.ink",
      entryIsExplicit: true,
    });
  });
});
