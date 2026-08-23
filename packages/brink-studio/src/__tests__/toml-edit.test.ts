/**
 * Comment-preserving structured edits for brink.toml (#3015): targeted
 * line operations, never parse-and-reserialize — the round-trip trap the
 * issue names is "rewriting the file from a parsed model silently
 * discards an author's comments".
 */
import { describe, expect, it } from "vitest";
import { getTomlString, setTomlString } from "@brink/studio-store";

const SOURCE = `# My story's config
[project]
# the entry point
entry = "main.ink"
dialect = "strict-ink"

[lints]
E063 = "allow"
`;

describe("getTomlString", () => {
  it("reads simple string values from the named table", () => {
    expect(getTomlString(SOURCE, "project", "entry")).toBe("main.ink");
    expect(getTomlString(SOURCE, "project", "dialect")).toBe("strict-ink");
  });
  it("returns null for absent keys, absent tables, and other tables' keys", () => {
    expect(getTomlString(SOURCE, "project", "conventions")).toBeNull();
    expect(getTomlString(SOURCE, "story", "entry")).toBeNull();
    expect(getTomlString(SOURCE, "project", "E063")).toBeNull();
  });
  it("reads literal (single-quoted) strings and unescapes basic ones", () => {
    expect(getTomlString(`[project]\nentry = 'a.ink'\n`, "project", "entry")).toBe("a.ink");
    expect(getTomlString(`[project]\nentry = "a\\"b.ink"\n`, "project", "entry")).toBe('a"b.ink');
  });
});

describe("setTomlString", () => {
  it("rewrites an existing key in place, preserving every other line", () => {
    const updated = setTomlString(SOURCE, "project", "entry", "chapter1.ink");
    expect(updated).toBe(SOURCE.replace('entry = "main.ink"', 'entry = "chapter1.ink"'));
    // The comments and the [lints] table survived verbatim.
    expect(updated).toContain("# My story's config");
    expect(updated).toContain("# the entry point");
    expect(updated).toContain('E063 = "allow"');
  });

  it("inserts a missing key after the table's last key line", () => {
    const updated = setTomlString(SOURCE, "project", "conventions", "screenplay.brink");
    expect(updated).toContain('dialect = "strict-ink"\nconventions = "screenplay.brink"\n');
  });

  it("appends the whole table when it does not exist", () => {
    const updated = setTomlString("# just a comment\n", "project", "entry", "main.ink");
    expect(updated).toBe('# just a comment\n[project]\nentry = "main.ink"\n');
  });

  it("removes a key on null, and removal of an absent key is a no-op", () => {
    const removed = setTomlString(SOURCE, "project", "dialect", null);
    expect(removed).not.toContain("dialect");
    expect(removed).toContain('entry = "main.ink"');
    expect(setTomlString(SOURCE, "project", "conventions", null)).toBe(SOURCE);
    expect(setTomlString(SOURCE, "story", "entry", null)).toBe(SOURCE);
  });

  it("escapes quotes and backslashes in written values", () => {
    const updated = setTomlString("[project]\n", "project", "entry", 'we"ird\\name.ink');
    expect(getTomlString(updated, "project", "entry")).toBe('we"ird\\name.ink');
  });
});
