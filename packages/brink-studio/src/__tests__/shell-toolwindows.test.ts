/**
 * @brink/studio-shell unit tests — tool-window registry, shell layout store,
 * and generated view-toggle commands (shell issue 1.3 / #80, spec §5.2–5.3,
 * §7.1).
 */

import { describe, expect, it, vi } from "vitest";
import {
  CommandRegistry,
  createShellLayoutStore,
  isToolWindowOpen,
  registerViewToggleCommands,
  ToolWindowRegistry,
  type Dock,
  type Section,
  type ToolWindowDescriptor,
} from "@brink/studio-shell";

function desc(
  id: string,
  dock: Dock,
  section: Section,
  defaultOpen = false,
  overrides: Partial<ToolWindowDescriptor> = {},
): ToolWindowDescriptor {
  return {
    id,
    title: id.charAt(0).toUpperCase() + id.slice(1),
    icon: null,
    defaultPlacement: { dock, section },
    defaultOpen,
    component: () => null,
    ...overrides,
  };
}

// The studio's default inventory (spec §4): binder open left/start, player
// open right/start, state closed right/end, program closed bottom/start.
function studioDescriptors(): ToolWindowDescriptor[] {
  return [
    desc("binder", "left", "start", true),
    desc("player", "right", "start", true),
    desc("state", "right", "end"),
    desc("program", "bottom", "start"),
  ];
}

function seededStore() {
  const store = createShellLayoutStore();
  store.getState().syncFromRegistry(studioDescriptors());
  return store;
}

// ── ToolWindowRegistry ──────────────────────────────────────────────

describe("ToolWindowRegistry", () => {
  it("registers, gets, and lists in registration order", () => {
    const registry = new ToolWindowRegistry();
    registry.register(desc("binder", "left", "start"));
    registry.register(desc("player", "right", "start"));

    expect(registry.list().map((d) => d.id)).toEqual(["binder", "player"]);
    expect(registry.get("binder")?.title).toBe("Binder");
    expect(registry.get("nope")).toBeUndefined();
  });

  it("rejects duplicate ids", () => {
    const registry = new ToolWindowRegistry();
    registry.register(desc("binder", "left", "start"));
    expect(() => registry.register(desc("binder", "right", "end"))).toThrow(/duplicate/);
  });

  it("rejects host-reserved ids for built-ins", () => {
    const registry = new ToolWindowRegistry();
    expect(() => registry.register(desc("host.acme.panel", "left", "start"))).toThrow(
      /reserved/,
    );
  });

  it("fires change events on register and unregister", () => {
    const registry = new ToolWindowRegistry();
    const listener = vi.fn();
    const unsubscribe = registry.onDidChange(listener);

    const dispose = registry.register(desc("binder", "left", "start"));
    expect(listener).toHaveBeenCalledTimes(1);

    dispose();
    expect(listener).toHaveBeenCalledTimes(2);
    expect(registry.list()).toEqual([]);

    dispose(); // double-dispose is a no-op
    expect(listener).toHaveBeenCalledTimes(2);

    unsubscribe();
    registry.register(desc("player", "right", "start"));
    expect(listener).toHaveBeenCalledTimes(2);
  });
});

// ── Shell layout store ──────────────────────────────────────────────

describe("ShellLayoutStore seeding", () => {
  it("seeds placements from defaultPlacement and open from defaultOpen", () => {
    const store = seededStore();
    const s = store.getState();

    expect(s.placements).toEqual({
      binder: { dock: "left", section: "start" },
      player: { dock: "right", section: "start" },
      state: { dock: "right", section: "end" },
      program: { dock: "bottom", section: "start" },
    });
    expect(s.open).toEqual({
      "left.start": "binder",
      "left.end": null,
      "right.start": "player",
      "right.end": null,
      "bottom.start": null,
      "bottom.end": null,
    });
  });

  it("first registered defaultOpen view per section wins", () => {
    const store = createShellLayoutStore();
    store.getState().syncFromRegistry([
      desc("a", "left", "start", true),
      desc("b", "left", "start", true),
    ]);
    expect(store.getState().open["left.start"]).toBe("a");
  });

  it("re-sync preserves user state and only seeds newly-seen ids", () => {
    const store = seededStore();
    store.getState().toggleToolWindow("player"); // user closes player
    store.getState().moveToolWindow("state", "bottom", "end");

    // Same registry plus a new defaultOpen window in right/start.
    store
      .getState()
      .syncFromRegistry([...studioDescriptors(), desc("problems", "right", "start", true)]);

    const s = store.getState();
    expect(s.open["right.start"]).toBe("problems"); // new id seeds the empty section
    expect(s.placements.state).toEqual({ dock: "bottom", section: "end" }); // user move kept
    expect(isToolWindowOpen(s, "player")).toBe(false); // player does NOT reopen
  });

  it("drops removed ids from placements and open", () => {
    const store = seededStore();
    store.getState().syncFromRegistry(studioDescriptors().filter((d) => d.id !== "binder"));

    const s = store.getState();
    expect(s.placements.binder).toBeUndefined();
    expect(s.open["left.start"]).toBeNull();
    expect(s.open["right.start"]).toBe("player");
  });
});

describe("ShellLayoutStore toggling (wide tier)", () => {
  it("toggles a window closed and open again", () => {
    const store = seededStore();
    store.getState().toggleToolWindow("binder");
    expect(store.getState().open["left.start"]).toBeNull();
    store.getState().toggleToolWindow("binder");
    expect(store.getState().open["left.start"]).toBe("binder");
  });

  it("opening a window closes the section's previous occupant", () => {
    const store = createShellLayoutStore();
    store.getState().syncFromRegistry([
      desc("a", "left", "start", true),
      desc("b", "left", "start"),
    ]);

    store.getState().toggleToolWindow("b");
    const s = store.getState();
    expect(s.open["left.start"]).toBe("b");
    expect(isToolWindowOpen(s, "a")).toBe(false);
  });

  it("ignores unknown ids", () => {
    const store = seededStore();
    const before = store.getState().open;
    store.getState().toggleToolWindow("nope");
    expect(store.getState().open).toEqual(before);
  });
});

describe("ShellLayoutStore moveToolWindow", () => {
  it("re-docks a closed window without opening it", () => {
    const store = seededStore();
    store.getState().moveToolWindow("program", "right", "end");

    const s = store.getState();
    expect(s.placements.program).toEqual({ dock: "right", section: "end" });
    expect(s.open["right.end"]).toBeNull();
    expect(s.open["bottom.start"]).toBeNull();
  });

  it("keeps an open window open in its new section, displacing the occupant", () => {
    const store = seededStore();
    store.getState().moveToolWindow("binder", "right", "start");

    const s = store.getState();
    expect(s.placements.binder).toEqual({ dock: "right", section: "start" });
    expect(s.open["left.start"]).toBeNull();
    expect(s.open["right.start"]).toBe("binder"); // player displaced
    expect(isToolWindowOpen(s, "player")).toBe(false);
  });

  it("is a no-op when the target equals the current placement", () => {
    const store = seededStore();
    const before = store.getState();
    store.getState().moveToolWindow("binder", "left", "start");
    expect(store.getState().placements).toEqual(before.placements);
    expect(store.getState().open).toEqual(before.open);
  });
});

describe("ShellLayoutStore tiers and transient presentation", () => {
  it("setTier dismisses drawers and the narrow overlay", () => {
    const store = seededStore();
    store.getState().setTier("medium");
    store.getState().setDrawerOpen("left", true);
    store.getState().setNarrowView("player");

    store.getState().setTier("wide");
    const s = store.getState();
    expect(s.tier).toBe("wide");
    expect(s.drawers).toEqual({ left: false, right: false });
    expect(s.narrowView).toBeNull();
    // Layout state itself is untouched — presentation only.
    expect(s.open["left.start"]).toBe("binder");
  });

  it("medium: toggling an open-but-hidden side window reveals its drawer, then closes", () => {
    const store = seededStore();
    store.getState().setTier("medium"); // binder open, drawer dismissed

    store.getState().toggleToolWindow("binder");
    expect(store.getState().drawers.left).toBe(true); // revealed, not closed
    expect(store.getState().open["left.start"]).toBe("binder");

    store.getState().toggleToolWindow("binder");
    expect(store.getState().open["left.start"]).toBeNull(); // now closes
    expect(store.getState().drawers.left).toBe(false);
  });

  it("narrow: toggling a right-dock window targets the overlay", () => {
    const store = seededStore();
    store.getState().setTier("narrow"); // player open, overlay dismissed

    store.getState().toggleToolWindow("player");
    expect(store.getState().narrowView).toBe("player"); // revealed

    store.getState().toggleToolWindow("player");
    expect(store.getState().open["right.start"]).toBeNull();
    expect(store.getState().narrowView).toBeNull();

    store.getState().toggleToolWindow("state");
    expect(store.getState().open["right.end"]).toBe("state");
    expect(store.getState().narrowView).toBe("state"); // opening reveals
  });

  it("setDockSize records rounded pixel sizes and rejects garbage", () => {
    const store = seededStore();
    store.getState().setDockSize("left", 301.6);
    expect(store.getState().dockSizes.left).toBe(302);
    store.getState().setDockSize("left", -5);
    store.getState().setDockSize("left", Number.NaN);
    expect(store.getState().dockSizes.left).toBe(302);
  });
});

// ── Generated view-toggle commands ──────────────────────────────────

describe("registerViewToggleCommands", () => {
  it("generates ids, titles, and Mod-1…9 by registration order", () => {
    const commands = new CommandRegistry();
    const store = seededStore();
    registerViewToggleCommands(commands, studioDescriptors(), store);

    const list = commands.list();
    expect(list.map((c) => c.id)).toEqual([
      "view.toggle.binder",
      "view.toggle.player",
      "view.toggle.state",
      "view.toggle.program",
    ]);
    expect(list.map((c) => c.keybinding)).toEqual(["Mod-1", "Mod-2", "Mod-3", "Mod-4"]);
    expect(commands.get("view.toggle.state")?.title).toBe("View: Toggle State");
  });

  it("leaves the tenth and later windows unbound", () => {
    const commands = new CommandRegistry();
    const store = createShellLayoutStore();
    const many = Array.from({ length: 10 }, (_, i) => desc(`w${i}`, "left", "start"));
    store.getState().syncFromRegistry(many);
    registerViewToggleCommands(commands, many, store);

    expect(commands.get("view.toggle.w8")?.keybinding).toBe("Mod-9");
    expect(commands.get("view.toggle.w9")?.keybinding).toBeUndefined();
  });

  it("dispatch toggles the layout store", () => {
    const commands = new CommandRegistry();
    const store = seededStore();
    registerViewToggleCommands(commands, studioDescriptors(), store);

    expect(commands.dispatch("view.toggle.state")).toBe(true);
    expect(store.getState().open["right.end"]).toBe("state");
    commands.dispatch("view.toggle.state");
    expect(store.getState().open["right.end"]).toBeNull();
  });

  it("the disposer unregisters every generated command", () => {
    const commands = new CommandRegistry();
    const store = seededStore();
    const dispose = registerViewToggleCommands(commands, studioDescriptors(), store);
    dispose();
    expect(commands.list()).toEqual([]);
    expect(commands.dispatch("view.toggle.binder")).toBe(false);
  });
});
