/**
 * The keymap editor's model.
 *
 * The rule under test is the rebinding ruling (2026-08-30): binding a chord
 * another command holds DISPLACES it and reports who, because `Keymap`'s
 * `byChord` is a `Map<chordId, commandId>` — two commands holding one chord
 * means the last registered silently wins and the other is dead with
 * nothing reporting it. An editor able to produce that state would let an
 * author configure something that does not work.
 */
import { describe, expect, it } from "vitest";
import {
  bindChord,
  chordId,
  chordOwner,
  keymapRows,
  parseKeybinding,
  resetCommand,
  serializeChord,
  unbindChord,
  type KeymapOverrides,
} from "@brink/studio-shell";

const COMMANDS = [
  { id: "search.find", title: "Search: Find in files", keybinding: "Mod-Shift-F" },
  { id: "search.symbol", title: "Search: Find symbol", keybinding: "Mod-T" },
  // Several defaults, because browsers reserve different chords (#107).
  { id: "view.palette", title: "View: Command palette", keybinding: ["Mod-Shift-P", "F1"] },
  { id: "editor.fold", title: "Editor: Fold all" },
];

const chord = (binding: string) => {
  const parsed = parseKeybinding(binding);
  expect(parsed, `${binding} should parse`).not.toBeNull();
  return parsed!;
};

describe("serializeChord", () => {
  it("round-trips every chord shape the capture UI can produce", () => {
    // `chordId` is the LOOKUP spelling and does not parse back; this is the
    // storage spelling, so a captured key must survive a save/load cycle.
    for (const binding of [
      "Mod-K",
      "Mod-Shift-P",
      "Alt-Enter",
      "Mod-Alt-Shift-B",
      "F5",
      "Shift-F11",
      "Mod-Minus",
      "Mod-=",
      "Mod-Shift-=",
      "Mod-Space",
    ]) {
      const original = chord(binding);
      const round = chord(serializeChord(original));
      expect(chordId(round), `${binding} did not round-trip`).toBe(chordId(original));
    }
  });

  it("spells a captured key the way the shipped defaults spell it", () => {
    // `chordFromEvent` lowercases, so without this the escape-hatch JSON
    // would hold "Mod-k" beside a default written "Mod-Shift-F".
    expect(serializeChord(chord("Mod-K"))).toBe("Mod-K");
    expect(serializeChord(chord("Shift-F11"))).toBe("Shift-F11");
    expect(serializeChord(chord("Alt-Enter"))).toBe("Alt-Enter");
  });

  it("spells keys that cannot be written literally", () => {
    // "Mod--" splits into an empty segment and is unparsable — the reason
    // KEY_ALIASES exists in the first place.
    expect(serializeChord(chord("Mod-Minus"))).toBe("Mod-Minus");
    expect(parseKeybinding(serializeChord(chord("Mod-Minus")))).not.toBeNull();
  });
});

describe("rows", () => {
  it("splits the category out of the palette title", () => {
    const find = keymapRows(COMMANDS, {}).find((r) => r.id === "search.find")!;
    expect(find.category).toBe("Search");
    expect(find.name).toBe("Find in files");
  });

  it("keeps every binding of a multi-default command", () => {
    // Flattening to one would drop the alternate that exists because a
    // browser eats the primary.
    const row = keymapRows(COMMANDS, {}).find((r) => r.id === "view.palette")!;
    expect(row.chords.map(chordId)).toEqual([
      chordId(chord("Mod-Shift-P")),
      chordId(chord("F1")),
    ]);
  });

  it("reports a command with no binding as unbound, not custom", () => {
    expect(keymapRows(COMMANDS, {}).find((r) => r.id === "editor.fold")!.source).toBe("unbound");
  });

  it("reads an override equal to the defaults as Default, not Custom", () => {
    // Ordinary editing produces one; a row reading "Custom" with a reset
    // button that changes nothing is a lie about the state.
    const rows = keymapRows(COMMANDS, { "search.find": "Mod-Shift-F" });
    expect(rows.find((r) => r.id === "search.find")!.source).toBe("default");
  });
});

describe("binding displaces the previous owner", () => {
  it("moves the chord and names who lost it", () => {
    const result = bindChord(COMMANDS, {}, "search.symbol", chord("Mod-Shift-F"));
    expect(result.displaced).toEqual({
      id: "search.find",
      title: "Search: Find in files",
      nowUnbound: true,
    });
    const rows = keymapRows(COMMANDS, result.overrides);
    expect(rows.find((r) => r.id === "search.find")!.chords).toEqual([]);
    expect(rows.find((r) => r.id === "search.symbol")!.chords.map(chordId)).toContain(
      chordId(chord("Mod-Shift-F")),
    );
  });

  it("takes only the colliding chord from a multi-binding command", () => {
    // The palette keeps F1 when it loses Mod-Shift-P — removing its whole
    // set would strip the browser-workaround alternates too.
    const result = bindChord(COMMANDS, {}, "search.symbol", chord("Mod-Shift-P"));
    expect(result.displaced?.id).toBe("view.palette");
    expect(result.displaced?.nowUnbound).toBe(false);
    const palette = keymapRows(COMMANDS, result.overrides).find((r) => r.id === "view.palette")!;
    expect(palette.chords.map(chordId)).toEqual([chordId(chord("F1"))]);
  });

  it("never leaves two commands holding one chord", () => {
    const result = bindChord(COMMANDS, {}, "search.symbol", chord("Mod-Shift-F"));
    const rows = keymapRows(COMMANDS, result.overrides);
    const target = chordId(chord("Mod-Shift-F"));
    const holders = rows.filter((r) => r.chords.some((x) => chordId(x) === target));
    expect(holders.map((r) => r.id)).toEqual(["search.symbol"]);
  });

  it("reports no displacement when the chord was free", () => {
    const result = bindChord(COMMANDS, {}, "editor.fold", chord("Mod-Alt-9"));
    expect(result.displaced).toBeNull();
    expect(
      keymapRows(COMMANDS, result.overrides).find((r) => r.id === "editor.fold")!.source,
    ).toBe("custom");
  });

  it("is a no-op when the command already holds the chord", () => {
    const result = bindChord(COMMANDS, {}, "search.find", chord("Mod-Shift-F"));
    expect(result.displaced).toBeNull();
    expect(result.overrides).toEqual({});
  });
});

describe("unbind and reset", () => {
  it("unbinding the last chord stores null, not an absent key", () => {
    // An absent key restores the DEFAULTS, the opposite of what was asked.
    const next = unbindChord(COMMANDS, {}, "search.find", chord("Mod-Shift-F"));
    expect(next["search.find"]).toBeNull();
    expect(keymapRows(COMMANDS, next).find((r) => r.id === "search.find")!.source).toBe("unbound");
  });

  it("reset drops the override and restores the shipped defaults", () => {
    const custom: KeymapOverrides = { "view.palette": "Mod-9" };
    const next = resetCommand(custom, "view.palette");
    expect(Object.prototype.hasOwnProperty.call(next, "view.palette")).toBe(false);
    expect(keymapRows(COMMANDS, next).find((r) => r.id === "view.palette")!.chords).toHaveLength(2);
  });

  it("editing back to the defaults leaves no residue in the overrides", () => {
    const off = unbindChord(COMMANDS, {}, "search.find", chord("Mod-Shift-F"));
    const back = bindChord(COMMANDS, off, "search.find", chord("Mod-Shift-F"));
    expect(back.overrides).toEqual({});
  });
});

describe("chordOwner", () => {
  it("finds the command a chord resolves to, honouring overrides", () => {
    expect(chordOwner(COMMANDS, {}, chord("F1"))).toBe("view.palette");
    expect(chordOwner(COMMANDS, { "view.palette": null }, chord("F1"))).toBeUndefined();
  });
});

describe("overridden is not the same as unbound", () => {
  it("a command that simply ships no binding has nothing to reset", () => {
    // Keyed on `source` alone, such a row offers a reset button that does
    // nothing and counts itself among the author's customisations.
    const row = keymapRows(COMMANDS, {}).find((r) => r.id === "editor.fold")!;
    expect(row.source).toBe("unbound");
    expect(row.overridden).toBe(false);
  });

  it("a command the author unbound does have something to reset", () => {
    const off = unbindChord(COMMANDS, {}, "search.find", chord("Mod-Shift-F"));
    const row = keymapRows(COMMANDS, off).find((r) => r.id === "search.find")!;
    expect(row.source).toBe("unbound");
    expect(row.overridden).toBe(true);
  });
});
