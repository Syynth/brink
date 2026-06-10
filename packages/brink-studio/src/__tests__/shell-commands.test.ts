/**
 * @brink/studio-shell unit tests — command registry, keymap layer (override
 * merge), and the global key handler (shell issue 1.1, spec §6).
 */

import { describe, expect, it, vi } from "vitest";
import {
  attachKeyHandler,
  chordId,
  CommandRegistry,
  Keymap,
  KEYMAP_STORAGE_KEY,
  loadKeymapOverrides,
  parseKeybinding,
  type Command,
} from "@brink/studio-shell";

function cmd(id: string, overrides: Partial<Command> = {}): Command {
  return { id, title: id, run: () => {}, ...overrides };
}

describe("CommandRegistry", () => {
  it("registers, lists in order, and dispatches", () => {
    const registry = new CommandRegistry();
    const ran: unknown[] = [];
    registry.register(cmd("a.one", { run: (args) => void ran.push(args) }));
    registry.register(cmd("a.two"));

    expect(registry.list().map((c) => c.id)).toEqual(["a.one", "a.two"]);
    expect(registry.dispatch("a.one", 42)).toBe(true);
    expect(ran).toEqual([42]);
  });

  it("rejects duplicate ids", () => {
    const registry = new CommandRegistry();
    registry.register(cmd("a.one"));
    expect(() => registry.register(cmd("a.one"))).toThrow(/duplicate/);
  });

  it("rejects host-reserved ids for built-ins", () => {
    const registry = new CommandRegistry();
    expect(() => registry.register(cmd("host.acme.thing"))).toThrow(/reserved/);
  });

  it("respects `when` for dispatch and isEnabled", () => {
    const registry = new CommandRegistry();
    let enabled = false;
    const run = vi.fn();
    registry.register(cmd("a.gated", { when: () => enabled, run }));

    expect(registry.isEnabled("a.gated")).toBe(false);
    expect(registry.dispatch("a.gated")).toBe(false);
    expect(run).not.toHaveBeenCalled();

    enabled = true;
    expect(registry.isEnabled("a.gated")).toBe(true);
    expect(registry.dispatch("a.gated")).toBe(true);
    expect(run).toHaveBeenCalledOnce();
  });

  it("returns false for unknown ids", () => {
    expect(new CommandRegistry().dispatch("nope")).toBe(false);
  });

  it("unregisters via the returned disposer", () => {
    const registry = new CommandRegistry();
    const dispose = registry.register(cmd("a.one"));
    dispose();
    expect(registry.dispatch("a.one")).toBe(false);
  });
});

describe("parseKeybinding / chordId", () => {
  it("parses modifiers in any order, case-insensitively", () => {
    const chord = parseKeybinding("shift-MOD-p");
    expect(chord).toEqual({ key: "p", mod: true, shift: true, alt: false });
    expect(chordId(chord!)).toBe("mod+shift+p");
  });

  it("parses digit and named keys", () => {
    expect(parseKeybinding("Mod-1")?.key).toBe("1");
    expect(parseKeybinding("Escape")?.key).toBe("escape");
  });

  it("rejects malformed bindings", () => {
    expect(parseKeybinding("")).toBeNull();
    expect(parseKeybinding("Mod-")).toBeNull();
    expect(parseKeybinding("Mod-A-B")).toBeNull();
    expect(parseKeybinding("Mod")).toBeNull();
  });
});

describe("Keymap", () => {
  const commands = [
    { id: "a.one", keybinding: "Mod-J" },
    { id: "a.two", keybinding: "Mod-Shift-P" },
    { id: "a.unbound" },
  ];

  it("resolves default bindings", () => {
    const keymap = Keymap.fromCommands(commands);
    expect(keymap.resolveChord({ key: "j", mod: true, shift: false, alt: false })).toBe("a.one");
    expect(keymap.resolveChord({ key: "p", mod: true, shift: true, alt: false })).toBe("a.two");
  });

  it("override rebinds a command", () => {
    const keymap = Keymap.fromCommands(commands, { "a.one": "Mod-K" });
    expect(keymap.resolveChord({ key: "j", mod: true, shift: false, alt: false })).toBeUndefined();
    expect(keymap.resolveChord({ key: "k", mod: true, shift: false, alt: false })).toBe("a.one");
  });

  it("null override unbinds; unknown ids and bad bindings are ignored", () => {
    const keymap = Keymap.fromCommands(commands, {
      "a.one": null,
      "a.unknown": "Mod-X",
      "a.two": "Not--Valid-X-Y",
    });
    expect(keymap.resolveChord({ key: "j", mod: true, shift: false, alt: false })).toBeUndefined();
    expect(keymap.resolveChord({ key: "x", mod: true, shift: false, alt: false })).toBeUndefined();
    expect(keymap.resolveChord({ key: "p", mod: true, shift: true, alt: false })).toBeUndefined();
  });
});

describe("loadKeymapOverrides", () => {
  function storageWith(value: string | null): Pick<Storage, "getItem"> {
    return { getItem: (key) => (key === KEYMAP_STORAGE_KEY ? value : null) };
  }

  it("loads a valid payload, keeping only string/null values", () => {
    const overrides = loadKeymapOverrides(
      storageWith(JSON.stringify({ "a.one": "Mod-K", "a.two": null, "a.bad": 7 })),
    );
    expect(overrides).toEqual({ "a.one": "Mod-K", "a.two": null });
  });

  it("is lenient about garbage", () => {
    expect(loadKeymapOverrides(storageWith(null))).toEqual({});
    expect(loadKeymapOverrides(storageWith("not json"))).toEqual({});
    expect(loadKeymapOverrides(storageWith('["array"]'))).toEqual({});
    expect(
      loadKeymapOverrides({
        getItem: () => {
          throw new Error("storage disabled");
        },
      }),
    ).toEqual({});
  });
});

describe("attachKeyHandler", () => {
  function setup(when?: () => boolean) {
    const registry = new CommandRegistry();
    const run = vi.fn();
    registry.register(cmd("a.one", { keybinding: "Mod-J", when, run }));
    const keymap = Keymap.fromCommands(registry.list());
    const detach = attachKeyHandler(window, registry, keymap, { isMac: false });
    return { run, detach };
  }

  function press(init: KeyboardEventInit & { key: string }, target: EventTarget = window): KeyboardEvent {
    const event = new KeyboardEvent("keydown", { cancelable: true, bubbles: true, ...init });
    target.dispatchEvent(event);
    return event;
  }

  it("dispatches a bound chord and consumes the event", () => {
    const { run, detach } = setup();
    const event = press({ key: "j", ctrlKey: true });
    expect(run).toHaveBeenCalledOnce();
    expect(event.defaultPrevented).toBe(true);
    detach();
  });

  it("ignores unbound chords and already-handled events", () => {
    const { run, detach } = setup();
    press({ key: "k", ctrlKey: true });
    const handled = new KeyboardEvent("keydown", { key: "j", ctrlKey: true, cancelable: true });
    handled.preventDefault();
    window.dispatchEvent(handled);
    expect(run).not.toHaveBeenCalled();
    detach();
  });

  it("does not consume the event when the command is disabled", () => {
    const { run, detach } = setup(() => false);
    const event = press({ key: "j", ctrlKey: true });
    expect(run).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
    detach();
  });

  it("skips modifier-less chords from editable targets", () => {
    const registry = new CommandRegistry();
    const run = vi.fn();
    registry.register(cmd("a.plain", { keybinding: "F2", run }));
    const keymap = Keymap.fromCommands(registry.list());
    const detach = attachKeyHandler(window, registry, keymap, { isMac: false });

    const input = document.createElement("input");
    document.body.appendChild(input);
    press({ key: "F2" }, input);
    expect(run).not.toHaveBeenCalled();

    press({ key: "F2" }, document.body);
    expect(run).toHaveBeenCalledOnce();

    input.remove();
    detach();
  });

  it("stops listening after detach", () => {
    const { run, detach } = setup();
    detach();
    press({ key: "j", ctrlKey: true });
    expect(run).not.toHaveBeenCalled();
  });
});
