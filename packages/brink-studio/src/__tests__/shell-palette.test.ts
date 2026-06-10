/**
 * @brink/studio-shell unit tests — palette filtering, chord formatting,
 * registry change events, and keymap binding lookup (shell issue 1.2, §6/§7.7).
 */

import { describe, expect, it, vi } from "vitest";
import {
  CommandRegistry,
  filterCommands,
  formatChord,
  Keymap,
  parseKeybinding,
  type Command,
} from "@brink/studio-shell";

function cmd(id: string, title: string, overrides: Partial<Command> = {}): Command {
  return { id, title, run: () => {}, ...overrides };
}

describe("filterCommands", () => {
  const commands = [
    cmd("view.togglePlayer", "View: Toggle Player"),
    cmd("story.restart", "Story: Restart"),
    cmd("story.stop", "Story: Stop", { when: () => false }),
    cmd("palette.toggle", "Command Palette"),
  ];

  it("returns enabled commands in order for an empty query", () => {
    expect(filterCommands(commands, "").map((c) => c.id)).toEqual([
      "view.togglePlayer",
      "story.restart",
      "palette.toggle",
    ]);
  });

  it("matches case-insensitive subsequences and ranks compact matches first", () => {
    const ids = filterCommands(commands, "story").map((c) => c.id);
    expect(ids[0]).toBe("story.restart");
    expect(ids).not.toContain("story.stop"); // disabled
  });

  it("matches against ids too", () => {
    expect(filterCommands(commands, "togglep").map((c) => c.id)).toContain(
      "view.togglePlayer",
    );
  });

  it("drops non-matches", () => {
    expect(filterCommands(commands, "zzz")).toEqual([]);
  });
});

describe("formatChord", () => {
  const chord = parseKeybinding("Mod-Shift-P")!;

  it("uses mac glyphs on macOS", () => {
    expect(formatChord(chord, true)).toBe("⌘⇧P");
  });

  it("uses Ctrl+Shift+ elsewhere, capitalizing named keys", () => {
    expect(formatChord(chord, false)).toBe("Ctrl+Shift+P");
    expect(formatChord(parseKeybinding("Escape")!, false)).toBe("Escape");
  });
});

describe("CommandRegistry.onDidChange", () => {
  it("notifies on register and unregister, and unsubscribes", () => {
    const registry = new CommandRegistry();
    const listener = vi.fn();
    const unsubscribe = registry.onDidChange(listener);

    const dispose = registry.register(cmd("a.one", "One"));
    expect(listener).toHaveBeenCalledTimes(1);
    dispose();
    expect(listener).toHaveBeenCalledTimes(2);
    dispose(); // second disposal is a no-op
    expect(listener).toHaveBeenCalledTimes(2);

    unsubscribe();
    registry.register(cmd("a.two", "Two"));
    expect(listener).toHaveBeenCalledTimes(2);
  });
});

describe("Keymap.bindingFor", () => {
  it("returns the effective post-override chord", () => {
    const keymap = Keymap.fromCommands(
      [
        { id: "a.one", keybinding: "Mod-J" },
        { id: "a.two", keybinding: "Mod-K" },
      ],
      { "a.one": "Mod-Shift-L", "a.two": null },
    );
    expect(keymap.bindingFor("a.one")).toEqual({
      key: "l",
      mod: true,
      shift: true,
      alt: false,
    });
    expect(keymap.bindingFor("a.two")).toBeUndefined();
  });
});
