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
  getTomlStringArray,
  setTomlBool,
  setTomlString,
  setTomlStringArray,
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

/**
 * String arrays — the prose dictionary's storage.
 *
 * The scalar editors above model a key as one line. A dictionary outgrows
 * that: it is author-visible, hand-editable, and only ever gets longer, so
 * it is written one entry per line and must be readable back across them.
 */
describe("string arrays", () => {
  const WITH_DICT = `[prose]
dialect = "british"
dictionary = [
  "Griswold",
  "Kaelen",
]

[lints]
E063 = "allow"
`;

  it("reads a multi-line array", () => {
    expect(getTomlStringArray(WITH_DICT, "prose", "dictionary")).toEqual([
      "Griswold",
      "Kaelen",
    ]);
  });

  it("reads a single-line array, which a hand-edited file may well hold", () => {
    const src = `[prose]\ndictionary = ["Ada", "Bo"]\n`;
    expect(getTomlStringArray(src, "prose", "dictionary")).toEqual(["Ada", "Bo"]);
  });

  it("tells an absent key from an empty array", () => {
    // Different answers to different questions: null is "this project has
    // never had a dictionary", [] is "it has one and it is empty". The
    // settings view says something different for each.
    expect(getTomlStringArray(`[prose]\ndialect = "american"\n`, "prose", "dictionary")).toBeNull();
    expect(getTomlStringArray(`[prose]\ndictionary = []\n`, "prose", "dictionary")).toEqual([]);
  });

  it("does not mistake a comma inside a word for a separator", () => {
    const src = `[prose]\ndictionary = ["Smith, Jr.", "Bo"]\n`;
    expect(getTomlStringArray(src, "prose", "dictionary")).toEqual(["Smith, Jr.", "Bo"]);
  });

  it("ignores comments inside the array", () => {
    const src = `[prose]\ndictionary = [\n  "Ada", # the cook\n  # a note on its own line\n  "Bo",\n]\n`;
    expect(getTomlStringArray(src, "prose", "dictionary")).toEqual(["Ada", "Bo"]);
  });

  it("reads a bracket inside a string as content, not as the array's end", () => {
    const src = `[prose]\ndictionary = [\n  "a]b",\n  "Bo",\n]\n`;
    expect(getTomlStringArray(src, "prose", "dictionary")).toEqual(["a]b", "Bo"]);
  });

  it("reports an unterminated array as absent rather than guessing its end", () => {
    // Guessing would let a later write truncate the file at a boundary we
    // cannot see — the one thing a config writer must never do.
    const src = `[prose]\ndictionary = [\n  "Ada",\n`;
    expect(getTomlStringArray(src, "prose", "dictionary")).toBeNull();
  });

  it("replaces a multi-line array in place, leaving neighbours alone", () => {
    const next = setTomlStringArray(WITH_DICT, "prose", "dictionary", ["Ada", "Bo", "Cy"]);
    expect(getTomlStringArray(next, "prose", "dictionary")).toEqual(["Ada", "Bo", "Cy"]);
    expect(getTomlString(next, "prose", "dialect")).toBe("british");
    expect(tomlTableKeys(next, "lints")).toEqual(["E063"]);
  });

  it("shrinking the array does not leave the old entries behind", () => {
    // The failure mode of a range-replace that measures the range wrong.
    const next = setTomlStringArray(WITH_DICT, "prose", "dictionary", ["Ada"]);
    expect(next).not.toContain("Kaelen");
    expect(next).not.toContain("Griswold");
    expect(getTomlStringArray(next, "prose", "dictionary")).toEqual(["Ada"]);
  });

  it("creates the table when it is missing", () => {
    const next = setTomlStringArray(`[project]\nentry = "main.ink"\n`, "prose", "dictionary", ["Ada"]);
    expect(getTomlStringArray(next, "prose", "dictionary")).toEqual(["Ada"]);
    expect(getTomlString(next, "project", "entry")).toBe("main.ink");
  });

  it("adds the key to an existing table without disturbing its comments", () => {
    const src = `# project config\n[prose]\n# which English\ndialect = "british"\n`;
    const next = setTomlStringArray(src, "prose", "dictionary", ["Ada"]);
    expect(next).toContain("# project config");
    expect(next).toContain("# which English");
    expect(getTomlString(next, "prose", "dialect")).toBe("british");
    expect(getTomlStringArray(next, "prose", "dictionary")).toEqual(["Ada"]);
  });

  it("round-trips a word containing a quote", () => {
    const next = setTomlStringArray(`[prose]\n`, "prose", "dictionary", ['O"Hara', "Bo"]);
    expect(getTomlStringArray(next, "prose", "dictionary")).toEqual(['O"Hara', "Bo"]);
  });

  it("removes the key when given null", () => {
    const next = setTomlStringArray(WITH_DICT, "prose", "dictionary", null);
    expect(getTomlStringArray(next, "prose", "dictionary")).toBeNull();
    expect(getTomlString(next, "prose", "dialect")).toBe("british");
  });

  it("writes an empty array rather than removing the key", () => {
    // Emptying the dictionary is a decision the author made; dropping the
    // key would present it back to them as "never configured".
    const next = setTomlStringArray(WITH_DICT, "prose", "dictionary", []);
    expect(getTomlStringArray(next, "prose", "dictionary")).toEqual([]);
  });

  it("overwrites a scalar sitting under the same key", () => {
    const src = `[prose]\ndictionary = "oops"\n`;
    const next = setTomlStringArray(src, "prose", "dictionary", ["Ada"]);
    expect(getTomlStringArray(next, "prose", "dictionary")).toEqual(["Ada"]);
    expect(next).not.toContain("oops");
  });

  it("stays one-entry-per-line at every size, so adding a word is a one-line diff", () => {
    const one = setTomlStringArray(`[prose]\n`, "prose", "dictionary", ["Ada"]);
    const two = setTomlStringArray(one, "prose", "dictionary", ["Ada", "Bo"]);
    const added = two.split("\n").filter((l) => !one.split("\n").includes(l));
    expect(added).toEqual(['  "Bo",']);
  });
});
