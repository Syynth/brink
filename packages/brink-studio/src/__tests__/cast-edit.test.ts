/**
 * `[cast]` edits (decision log 2026-10-10): the web twin of the Rust
 * `ConfigDocument` cast helpers, with the same rules — names match
 * ignoring case, comments survive, and removing the last speaker gives
 * back the text as it was.
 */
import { describe, expect, it } from "vitest";
import { castEntries, removeCastMember, setCastColor } from "@brink/studio-store";

const TEXT = '# My story\n[project]\nentry = "main.ink"\n';

describe("[cast] edits", () => {
  it("adds, updates and removes a speaker in place", () => {
    const added = setCastColor(setCastColor(TEXT, "Mara", "#d97757"), "Old Tom", "#55bbff");
    expect(added.startsWith(TEXT)).toBe(true);
    expect(added).toContain('[cast.Mara]\ncolor = "#d97757"');
    expect(added).toContain('[cast."Old Tom"]\ncolor = "#55bbff"');
    expect(castEntries(added)).toEqual([
      { name: "Mara", color: "#d97757" },
      { name: "Old Tom", color: "#55bbff" },
    ]);

    // Editing `MARA` updates the existing entry and keeps its comment.
    const commented = added.replace('"#d97757"', '"#d97757" # warm');
    const updated = setCastColor(commented, "MARA", "#aa0000");
    expect(updated).toContain('[cast.Mara]\ncolor = "#aa0000" # warm');
    expect(updated).not.toContain("MARA");

    const removed = removeCastMember(removeCastMember(updated, "old tom"), "Mara");
    expect(removed).toBe(TEXT);
    expect(removeCastMember(TEXT, "Jonah")).toBe(TEXT);
  });

  it("removes a speaker between tables without gluing them together", () => {
    const text = '[project]\nentry = "main.ink"\n\n[cast.Mara]\ncolor = "#123"\n\n[lints]\nE063 = "allow"\n';
    expect(removeCastMember(text, "mara")).toBe('[project]\nentry = "main.ink"\n\n[lints]\nE063 = "allow"\n');
  });

  it("reads quoted names and a colour with a trailing comment", () => {
    const text = "[cast.'Old Tom'] # the ferryman\ncolor = '#5bf' # pale\n";
    expect(castEntries(text)).toEqual([{ name: "Old Tom", color: "#5bf" }]);
  });

  it("adds a colour line to a speaker's table that has none", () => {
    const text = "[cast.Mara]\n# colour to come\n";
    expect(setCastColor(text, "mara", "#d97757")).toBe('[cast.Mara]\n# colour to come\ncolor = "#d97757"\n');
  });
});
