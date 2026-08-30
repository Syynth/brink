/**
 * Every command registered with a keybinding reaches the keymap editor.
 *
 * The keymap table lists `CommandRegistry.list()`, and the live keymap is
 * `Keymap.fromCommands(commands.list(), overrides)` — one source. That is
 * the property worth pinning rather than assuming: a feature that wired a
 * key straight to a keydown handler instead of registering a command would
 * work perfectly and be invisible in Settings, unrebindable, and outside
 * conflict detection. Nothing in the type system prevents it.
 *
 * The debugger's F-row (#3303/#3327) is the concrete case — it landed while
 * the keymap editor was being designed, and it registers commands, so it is
 * covered for free. This test is what keeps that true.
 */
import { describe, expect, it } from "vitest";
import { CommandRegistry, chordId, keymapRows, parseKeybinding } from "@brink/studio-shell";
import { createStudioStore } from "@brink/studio-store";
import { registerDebugCommands } from "../debug-commands";
import { registerStoryCommands } from "../story-commands";

function registry() {
  const commands = new CommandRegistry();
  const store = createStudioStore();
  registerDebugCommands(commands, store, () => "main.ink");
  registerStoryCommands(commands, store);
  return commands;
}

describe("the keymap table", () => {
  it("lists every registered command that carries a keybinding", () => {
    const commands = registry();
    const bound = commands.list().filter((c) => c.keybinding !== undefined);
    expect(bound.length, "no bound commands registered — check the fixtures").toBeGreaterThan(0);

    const rows = keymapRows(commands.list(), {});
    for (const command of bound) {
      const row = rows.find((r) => r.id === command.id);
      expect(row, `${command.id} is bound but absent from the keymap table`).toBeDefined();
      expect(row!.chords.length, `${command.id} shows no chord`).toBeGreaterThan(0);
    }
  });

  it("shows the debugger's F-row bindings", () => {
    // Named explicitly: these are the keys an author is most likely to want
    // rebound, since F-keys collide with OS and browser assignments.
    const rows = keymapRows(registry().list(), {});
    const shown = new Set(rows.flatMap((r) => r.chords.map(chordId)));
    for (const binding of ["F5", "F6", "F9", "F10", "F11", "Shift-F11"]) {
      const chord = parseKeybinding(binding);
      expect(chord, `${binding} should parse`).not.toBeNull();
      expect(shown.has(chordId(chord!)), `${binding} is not in the keymap table`).toBe(true);
    }
  });

  it("has no two commands sharing a chord", () => {
    // `Keymap.byChord` is a plain Map.set, so a duplicate silently drops one
    // of them. There is none today; this fails the moment one appears.
    const rows = keymapRows(registry().list(), {});
    const owners = new Map<string, string[]>();
    for (const row of rows) {
      for (const chord of row.chords) {
        const id = chordId(chord);
        owners.set(id, [...(owners.get(id) ?? []), row.title]);
      }
    }
    const clashes = [...owners.entries()].filter(([, who]) => who.length > 1);
    expect(clashes.map(([chord, who]) => `${chord}: ${who.join(" / ")}`)).toEqual([]);
  });
});
