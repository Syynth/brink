/**
 * The file-anchored open model's pure decision logic (#3021, ruled
 * 2026-08-23): path → door classification, recents display, the conflict
 * banner model, and the New Project entry-name validation mirror.
 */
import { describe, expect, it } from "vitest";
import {
  anchorForPath,
  buildConflictModel,
  recentDisplayFor,
  recentKindFor,
  relativeConfigPath,
  resolveBootAction,
  validateEntryName,
} from "../project-open.js";

describe("recentKindFor", () => {
  it("classifies by path shape alone — no fs round-trip, no recents migration", () => {
    expect(recentKindFor("/stories/harbour/brink.toml")).toBe("toml");
    expect(recentKindFor("/drafts/nightjar/prologue.ink")).toBe("ink");
    expect(recentKindFor("/stories/harbour")).toBe("folder");
    // A non-config toml is not a door, but it is also not a folder —
    // anchorForPath rejects it before this ever matters.
    expect(recentKindFor("/stories/Cargo.toml")).toBe("folder");
  });
});

describe("anchorForPath", () => {
  it("opens a brink.toml on the toml door: parent as root, config's entry governs", () => {
    const anchor = anchorForPath("/stories/harbour/brink.toml");
    expect(anchor).toEqual({
      kind: "toml",
      root: "/stories/harbour",
      entryFile: null,
      entryIsExplicit: false,
      conflictProbe: null,
      recentPath: "/stories/harbour/brink.toml",
    });
  });

  it("opens a .ink on the story door: explicit entry, conflict probe armed", () => {
    const anchor = anchorForPath("/drafts/nightjar/prologue.ink");
    expect(anchor).toEqual({
      kind: "ink",
      root: "/drafts/nightjar",
      entryFile: "prologue.ink",
      entryIsExplicit: true,
      conflictProbe: "/drafts/nightjar/prologue.ink",
      recentPath: "/drafts/nightjar/prologue.ink",
    });
  });

  it("rejects a .toml that is not brink.toml with a reason", () => {
    const anchor = anchorForPath("/stories/Cargo.toml");
    expect("error" in anchor).toBe(true);
  });

  it("keeps the legacy folder door for .brink (native file-anchoring deferred)", () => {
    const anchor = anchorForPath("/native/mod.brink");
    expect(anchor).toMatchObject({
      kind: "folder",
      root: "/native",
      entryFile: "mod.brink",
      entryIsExplicit: false,
      recentPath: "/native",
    });
  });

  it("treats a bare folder path as the legacy door", () => {
    const anchor = anchorForPath("/stories/harbour");
    expect(anchor).toMatchObject({
      kind: "folder",
      root: "/stories/harbour",
      entryFile: null,
      entryIsExplicit: false,
      recentPath: "/stories/harbour",
    });
  });
});

describe("recentDisplayFor", () => {
  it("shows the file name for the file doors and the folder name for legacy entries", () => {
    expect(recentDisplayFor("/Users/b/stories/harbour/brink.toml", "/Users/b")).toEqual({
      kind: "toml",
      name: "brink.toml",
      detail: "~/stories/harbour",
    });
    expect(recentDisplayFor("/Users/b/drafts/prologue.ink", "/Users/b")).toEqual({
      kind: "ink",
      name: "prologue.ink",
      detail: "~/drafts",
    });
    expect(recentDisplayFor("/Users/b/scratch", "/Users/b")).toEqual({
      kind: "folder",
      name: "scratch",
      detail: "~",
    });
  });

  it("leaves paths outside home uncontracted", () => {
    expect(recentDisplayFor("/srv/stories/x.ink", "/Users/b").detail).toBe("/srv/stories");
  });
});

describe("buildConflictModel", () => {
  const discovered = {
    configPath: "/repo/brink.toml",
    entry: "story/act2/chapter3.ink",
    openedIsEntry: true,
    walked: ["/repo/story/act2", "/repo/story"],
    warnings: [],
  };

  it("returns null when nothing governs", () => {
    expect(buildConflictModel("/repo/story/act2/chapter3.ink", null)).toBeNull();
  });

  it("builds the banner text pieces and the walk trace", () => {
    const model = buildConflictModel("/repo/story/act2/chapter3.ink", discovered);
    expect(model).not.toBeNull();
    if (model === null) return;
    expect(model.relConfig).toBe("../../brink.toml");
    expect(model.openedIsEntry).toBe(true);
    expect(model.trace).toHaveLength(4);
    expect(model.trace[0]).toMatchObject({ path: "chapter3.ink", note: "opened" });
    expect(model.trace[1]).toMatchObject({ path: "act2/", note: "no brink.toml" });
    expect(model.trace[3]).toMatchObject({
      found: true,
      note: "governs — entry = story/act2/chapter3.ink",
    });
  });

  it("a config beside the opened file has an empty walk and a bare relConfig", () => {
    const model = buildConflictModel("/repo/offcuts.ink", {
      ...discovered,
      entry: "main.ink",
      openedIsEntry: false,
      walked: [],
    });
    expect(model?.relConfig).toBe("brink.toml");
    expect(model?.trace).toHaveLength(2);
    expect(model?.openedIsEntry).toBe(false);
  });
});

describe("relativeConfigPath", () => {
  it("adds one ../ per walked directory", () => {
    expect(relativeConfigPath(0)).toBe("brink.toml");
    expect(relativeConfigPath(2)).toBe("../../brink.toml");
  });
});

describe("validateEntryName (mirror of the shell command's validator)", () => {
  it("accepts ordinary .ink names", () => {
    expect(validateEntryName("main.ink")).toBeNull();
    expect(validateEntryName("The Harbour.ink")).toBeNull();
  });
  it("rejects what the Rust validator rejects", () => {
    for (const bad of ["", ".ink", "main", "main.brink", "a/b.ink", "a\\b.ink", ".hidden.ink"]) {
      expect(validateEntryName(bad), bad).not.toBeNull();
    }
  });
});

describe("resolveBootAction (#3016)", () => {
  const base = {
    reopenLastProject: true,
    previousExitClean: true,
    osOpenHandled: false,
    recents: ["/stories/harbour/brink.toml", "/drafts/x.ink"],
  };
  it("reopens the most recent anchor when the preference is on after a clean exit", () => {
    expect(resolveBootAction(base)).toEqual({
      kind: "reopen",
      path: "/stories/harbour/brink.toml",
    });
  });
  it("a cold-start OS file-open always wins over auto-reopen", () => {
    expect(resolveBootAction({ ...base, osOpenHandled: true })).toEqual({ kind: "none" });
  });
  it("shows the plain landing when the preference is off or recents are empty", () => {
    expect(resolveBootAction({ ...base, reopenLastProject: false })).toEqual({ kind: "landing" });
    expect(resolveBootAction({ ...base, recents: [] })).toEqual({ kind: "landing" });
  });
  it("the crash guard suppresses reopen after an unclean exit, and says why", () => {
    const action = resolveBootAction({ ...base, previousExitClean: false });
    expect(action.kind).toBe("landing");
    if (action.kind === "landing") {
      expect(action.note).toContain("didn't exit cleanly");
    }
  });
});
