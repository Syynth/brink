/**
 * The editor's named actions as shell commands (editor-action-commands.ts).
 *
 * The property that matters end-to-end: the shell's overrides are the ONE
 * source of truth, and the editors' CodeMirror keymaps follow them — so
 * Settings ▸ Keymap can never show a chord the editor disagrees with.
 */
import { describe, expect, it, vi } from "vitest";
import { CommandRegistry, keymapRows, parseKeybinding } from "@brink/studio-shell";
import { EDITOR_ACTIONS, type EditorActionKeys } from "@brink-lang/editor";
import {
  chordToCm6Key,
  registerEditorActionCommands,
  type EditorActionHost,
} from "../editor-action-commands.js";

function harness(initial: Record<string, string | readonly string[] | null> = {}) {
  const commands = new CommandRegistry();
  const pushed: EditorActionKeys[] = [];
  const host: EditorActionHost = {
    runEditorAction: vi.fn(() => true),
    setEditorActionKeys: (keys) => pushed.push(keys),
  };
  let current = initial;
  const listeners = new Set<() => void>();
  const overrides = {
    get current() {
      return current;
    },
    onDidChange(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const set = (next: typeof current): void => {
    current = next;
    for (const l of listeners) l();
  };
  const dispose = registerEditorActionCommands(commands, host, overrides);
  return { commands, host, pushed, set, dispose };
}

describe("the editor-action commands", () => {
  it("registers all five, with the editor's shipped chords as defaults", () => {
    const { commands } = harness();
    const rows = keymapRows(commands.list(), {});
    for (const [id, action] of Object.entries(EDITOR_ACTIONS)) {
      const row = rows.find((r) => r.id === id);
      expect(row, `${id} missing from the keymap table`).toBeDefined();
      expect(row!.source).toBe("default");
      expect(row!.chords.length, `${id} shows no chord`).toBeGreaterThan(0);
      // The same spelling must parse in the SHELL's dialect too — that is
      // what lets the command declare the editor's default verbatim.
      expect(parseKeybinding(action.key), `${id}: ${action.key}`).not.toBeNull();
    }
  });

  it("pushes the current chords into the editors at registration", () => {
    // An author with saved rebinds from a previous session gets them at
    // mount, not at their first edit.
    const { pushed } = harness({ "editor.renameSymbol": "Mod-R" });
    expect(pushed).toHaveLength(1);
    expect(pushed[0]["editor.renameSymbol"]).toEqual(["Mod-r"]);
    // Untouched actions ride along at their defaults.
    expect(pushed[0]["editor.codeActions"]).toEqual(["Mod-."]);
  });

  it("rebroadcasts on every override change, in CM6 spelling", () => {
    const { pushed, set } = harness();
    set({ "editor.findReferences": ["Mod-F12", "Shift-Alt-F"] });
    const last = pushed.at(-1)!;
    expect(last["editor.findReferences"]).toEqual(["Mod-F12", "Alt-Shift-f"]);
  });

  it("broadcasts null for an explicit unbind", () => {
    const { pushed, set } = harness();
    set({ "editor.insertElement": null });
    expect(pushed.at(-1)!["editor.insertElement"]).toBeNull();
  });

  it("dispatches a command run into the focused editor", () => {
    const { commands, host } = harness();
    commands.dispatch("editor.renameSymbol");
    expect(host.runEditorAction).toHaveBeenCalledWith("editor.renameSymbol");
  });

  it("disposal deregisters and stops the sync", () => {
    const { commands, pushed, set, dispose } = harness();
    dispose();
    expect(commands.list().find((c) => c.id === "editor.renameSymbol")).toBeUndefined();
    const count = pushed.length;
    set({ "editor.renameSymbol": "Mod-K" });
    expect(pushed).toHaveLength(count);
  });
});

describe("chordToCm6Key", () => {
  it("keeps the dialect deltas straight", () => {
    const cases: [string, string][] = [
      ["F2", "F2"],
      ["Mod-Shift-A", "Mod-Shift-a"], // single letters go out lowercase
      ["Alt-Enter", "Alt-Enter"],
      ["Mod-.", "Mod-."],
      ["Mod-Minus", "Mod--"], // CM6's splitter is `-` not at end-of-string
      ["Mod-Space", "Mod-Space"],
      ["Shift-F11", "Shift-F11"],
      ["Mod-ArrowUp", "Mod-ArrowUp"],
    ];
    for (const [shell, cm6] of cases) {
      const chord = parseKeybinding(shell);
      expect(chord, shell).not.toBeNull();
      expect(chordToCm6Key(chord!), shell).toBe(cm6);
    }
  });
});
