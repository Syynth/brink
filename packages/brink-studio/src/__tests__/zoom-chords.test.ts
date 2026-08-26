/**
 * Editor zoom chords (⌘= / ⌘+ / ⌘−), ruled 2026-08-25.
 *
 * Two bugs made the decrease chord unreachable, both in the keybinding
 * layer rather than the command:
 *
 *  1. `parseKeybinding` splits on "-", so the natural spelling "Mod--"
 *     produced an empty segment and parsed as MALFORMED — the binding
 *     silently did not exist. Hence the `Minus` alias.
 *  2. Shift changes `event.key` for punctuation: ⌘+ reports "+", not "=",
 *     so a `Mod-Shift-=` binding never matched the chord the browser
 *     actually delivers. Shifted keys now fold onto the unshifted key on
 *     the same physical cap.
 */

import { describe, expect, it } from "vitest";
import { chordFromEvent, chordId, parseKeybinding } from "@brink/studio-shell";

const ev = (key: string, opts: { meta?: boolean; shift?: boolean } = {}) =>
  ({
    key,
    metaKey: opts.meta ?? true,
    ctrlKey: false,
    shiftKey: opts.shift ?? false,
    altKey: false,
  }) as KeyboardEvent;

describe("parseKeybinding punctuation", () => {
  it('"Mod--" is still malformed — the alias is the supported spelling', () => {
    expect(parseKeybinding("Mod--")).toBeNull();
  });

  it("Mod-Minus parses to the minus key", () => {
    expect(parseKeybinding("Mod-Minus")).toEqual({
      key: "-",
      mod: true,
      shift: false,
      alt: false,
    });
  });

  it("Plus and Equal both land on the physical = cap", () => {
    expect(parseKeybinding("Mod-Plus")?.key).toBe("=");
    expect(parseKeybinding("Mod-Equal")?.key).toBe("=");
    expect(parseKeybinding("Mod-=")?.key).toBe("=");
  });
});

describe("chordFromEvent shifted punctuation", () => {
  it("⌘+ folds onto = (with shift still recorded)", () => {
    const chord = chordFromEvent(ev("+", { shift: true }), true);
    expect(chord).toEqual({ key: "=", mod: true, shift: true, alt: false });
  });

  it("⌘_ folds onto -", () => {
    expect(chordFromEvent(ev("_", { shift: true }), true)?.key).toBe("-");
  });

  it("unshifted chords are unchanged", () => {
    expect(chordFromEvent(ev("="), true)?.key).toBe("=");
    expect(chordFromEvent(ev("-"), true)?.key).toBe("-");
  });
});

describe("the registered zoom bindings actually resolve", () => {
  // What mount.tsx registers, restated: if these ids stop matching the
  // chords the browser delivers, zoom silently stops working.
  const INCREASE = ["Mod-=", "Mod-Shift-="];
  const DECREASE = ["Mod-Minus", "Mod-Shift-Minus"];

  const idsFor = (bindings: string[]) =>
    bindings.map((b) => chordId(parseKeybinding(b)!));

  it("⌘= and ⌘+ both resolve to an increase binding", () => {
    const ids = idsFor(INCREASE);
    expect(ids).toContain(chordId(chordFromEvent(ev("="), true)!));
    expect(ids).toContain(chordId(chordFromEvent(ev("+", { shift: true }), true)!));
  });

  it("⌘- and ⌘_ both resolve to a decrease binding", () => {
    const ids = idsFor(DECREASE);
    expect(ids).toContain(chordId(chordFromEvent(ev("-"), true)!));
    expect(ids).toContain(chordId(chordFromEvent(ev("_", { shift: true }), true)!));
  });

  it("increase and decrease never collide", () => {
    for (const id of idsFor(INCREASE)) expect(idsFor(DECREASE)).not.toContain(id);
  });
});
