/**
 * The `[dialogue]` section as one owned block (#3410): section-level
 * rewrite, everything else byte-for-byte preserved, and the marker that
 * tells a hand-written or hand-edited section from the editor's own.
 */
import { describe, expect, it } from "vitest";
import {
  CONVENTIONS_MARKER,
  findDialogueSection,
  renderDialogueSection,
  setDialogueSection,
} from "@brink/studio-store";

const OTHER = `# My story's config
[project]
entry = "main.ink" # the entry

[lints]
E063 = "allow"
`;

const TABLE = renderDialogueSection({
  form: "table",
  table: {
    preset: "at-cue",
    runEndsAt: ["action", "choices"],
    elements: [
      { kind: "character", prefix: "@", suffix: ": ", glued: true, contentRole: "speaker" },
      { kind: "action", prefix: "> " },
    ],
  },
});

describe("renderDialogueSection", () => {
  it("renders the table form with the marker first and one [[dialogue.elements]] per row", () => {
    expect(TABLE.split("\n")[0].startsWith(CONVENTIONS_MARKER)).toBe(true);
    expect(TABLE).toContain('[dialogue]\npreset = "at-cue"\nrun-ends-at = ["action", "choices"]\n\n[[dialogue.elements]]\nkind = "character"\nprefix = "@"\nsuffix = ": "\nglued = true\ncontent-role = "speaker"\n\n[[dialogue.elements]]\nkind = "action"\nprefix = "> "');
    expect(TABLE.endsWith("\n")).toBe(false);
  });

  it("renders the file form", () => {
    const s = renderDialogueSection({ form: "file", file: "dialect.json" });
    expect(s.split("\n").slice(1)).toEqual(["[dialogue]", 'file = "dialect.json"']);
  });

  it("escapes quotes and backslashes in values", () => {
    const s = renderDialogueSection({ form: "table", table: { elements: [{ kind: "q", prefix: '"', suffix: "\\" }] } });
    expect(s).toContain('prefix = "\\""');
    expect(s).toContain('suffix = "\\\\"');
  });
});

describe("setDialogueSection / findDialogueSection", () => {
  it("appends after a blank line when the file has no section, and finds it back as the editor's", () => {
    const out = setDialogueSection(OTHER, TABLE);
    expect(out.startsWith(OTHER)).toBe(true);
    expect(out).toBe(`${OTHER}\n${TABLE}\n`);
    const found = findDialogueSection(out);
    expect(found?.owner).toBe("editor");
    expect(found?.text).toBe(TABLE);
  });

  it("replaces only the section — tables before and after survive byte-for-byte", () => {
    const withSection = `${OTHER}\n${TABLE}\n\n[player]\nfast = true # keep\n`;
    const next = renderDialogueSection({ form: "table", table: { preset: "at-cue" } });
    const out = setDialogueSection(withSection, next);
    expect(out).toBe(`${OTHER}\n${next}\n\n[player]\nfast = true # keep\n`);
  });

  it("removes the section and one separating blank line", () => {
    const withSection = `${OTHER}\n${TABLE}\n\n[player]\nfast = true\n`;
    expect(setDialogueSection(withSection, null)).toBe(`${OTHER}\n[player]\nfast = true\n`);
    expect(setDialogueSection(OTHER, null)).toBe(OTHER);
  });

  it("a section with no marker was written by hand", () => {
    const src = `${OTHER}\n[dialogue]\npreset = "at-cue"\n\n[[dialogue.elements]]\nkind = "action"\nprefix = ">"\n\n[player]\nx = 1\n`;
    const found = findDialogueSection(src);
    expect(found?.owner).toBe("hand");
    expect(found?.text).toBe('[dialogue]\npreset = "at-cue"\n\n[[dialogue.elements]]\nkind = "action"\nprefix = ">"');
  });

  it("an editor-written section edited since reads as edited", () => {
    const out = setDialogueSection(OTHER, TABLE).replace('prefix = "> "', 'prefix = ">> "');
    expect(findDialogueSection(out)?.owner).toBe("edited");
  });

  it("trailing whitespace on a line is not an edit", () => {
    const out = setDialogueSection(OTHER, TABLE).replace('prefix = "@"', 'prefix = "@"   ');
    expect(findDialogueSection(out)?.owner).toBe("editor");
  });

  it("a sub-table header of another table ends the section", () => {
    const src = `[dialogue]\npreset = "at-cue"\n[dialogue.extra]\nk = 1\n[dialogues]\nz = 2\n`;
    const found = findDialogueSection(src);
    expect(found?.text).toBe('[dialogue]\npreset = "at-cue"\n[dialogue.extra]\nk = 1');
  });
});
