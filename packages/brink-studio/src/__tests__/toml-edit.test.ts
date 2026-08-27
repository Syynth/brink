/**
 * Comment-preserving structured edits for brink.toml (#3015): targeted
 * line operations, never parse-and-reserialize — the round-trip trap the
 * issue names is "rewriting the file from a parsed model silently
 * discards an author's comments".
 */
import { describe, expect, it } from "vitest";
import {
  getTomlBool,
  getTomlString,
  setTomlBool,
  setTomlString,
  tomlTableKeys,
} from "@brink/studio-store";

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

describe("tomlTableKeys / bool values — what [lints] needs (#3148)", () => {
  const CONFIG = `[project]
entry = "main.ink"

# which diagnostics this project has decided about
[lints]
deny-warnings = false
E014 = "deny"
E033 = "allow"
`;

  it("lists a table's keys in file order", () => {
    // File order, not sorted: the Diagnostics section groups by category
    // itself, and re-ordering here would fight the file rather than the UI.
    expect(tomlTableKeys(CONFIG, "lints")).toEqual(["deny-warnings", "E014", "E033"]);
  });

  it("returns nothing for a table that is not there", () => {
    expect(tomlTableKeys('[project]\nentry = "main.ink"\n', "lints")).toEqual([]);
  });

  it("does not leak keys from a neighbouring table", () => {
    expect(tomlTableKeys(CONFIG, "project")).toEqual(["entry"]);
  });

  it("reads a bool, and does not read one as a string", () => {
    expect(getTomlBool(CONFIG, "lints", "deny-warnings")).toBe(false);
    // The reason this helper exists: a bare `true` is not a TOML string, so
    // reading it with getTomlString would make the key invisible to the
    // form while it is plainly set in the file.
    expect(getTomlString(CONFIG, "lints", "deny-warnings")).toBeNull();
  });

  it("writes and removes a bool without quoting it", () => {
    const on = setTomlBool(CONFIG, "lints", "deny-warnings", true);
    expect(on).toContain("deny-warnings = true");
    expect(on).not.toContain('deny-warnings = "true"');
    expect(getTomlBool(setTomlBool(on, "lints", "deny-warnings", null), "lints", "deny-warnings"))
      .toBeNull();
  });

  it("preserves the comment above the table when adding a code", () => {
    // The whole point of the line editors: an author's comments survive.
    const next = setTomlString(CONFIG, "lints", "E063", "warn");
    expect(next).toContain("# which diagnostics this project has decided about");
    expect(next).toContain('E063 = "warn"');
    expect(tomlTableKeys(next, "lints")).toEqual(["deny-warnings", "E014", "E033", "E063"]);
  });

  it("removing a code leaves the others alone", () => {
    const next = setTomlString(CONFIG, "lints", "E014", null);
    expect(tomlTableKeys(next, "lints")).toEqual(["deny-warnings", "E033"]);
    expect(next).toContain('E033 = "allow"');
  });
});
