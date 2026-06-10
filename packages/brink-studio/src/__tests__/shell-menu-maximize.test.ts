/**
 * Hamburger menu grouping + tool-window maximize (shell issue 2.5, spec §6,
 * §5.4).
 */

import { describe, expect, it } from "vitest";
import {
  createShellLayoutStore,
  groupCommandsForMenu,
  type Command,
} from "@brink/studio-shell";

function cmd(id: string, title: string, overrides: Partial<Command> = {}): Command {
  return { id, title, run: () => {}, ...overrides };
}

describe("groupCommandsForMenu", () => {
  it("groups by id prefix in first-appearance order, registration order within", () => {
    const groups = groupCommandsForMenu([
      cmd("story.restart", "Story: Restart"),
      cmd("view.toggle.binder", "View: Toggle Binder"),
      cmd("story.stop", "Story: Stop"),
      cmd("quickOpen.toggle", "Go to File or Symbol"),
    ]);
    expect(groups.map((g: { label: string }) => g.label)).toEqual(["Story", "View", "Quick Open"]);
    expect(groups[0]!.commands.map((c: { id: string }) => c.id)).toEqual(["story.restart", "story.stop"]);
  });

  it("excludes disabled commands; empty groups never appear", () => {
    const groups = groupCommandsForMenu([
      cmd("story.choose", "Story: Choose", { when: () => false }),
      cmd("view.toggle.player", "View: Toggle Player"),
    ]);
    expect(groups.map((g: { label: string }) => g.label)).toEqual(["View"]);
  });
});

describe("toggleMaximize", () => {
  const seed = (store: ReturnType<typeof createShellLayoutStore>) =>
    store.getState().syncFromRegistry([
      {
        id: "player",
        title: "Player",
        icon: null,
        defaultPlacement: { dock: "right", section: "start" },
        defaultOpen: true,
        component: () => null,
      },
    ]);

  it("maximizes a placed window and restores on re-toggle", () => {
    const store = createShellLayoutStore();
    seed(store);
    store.getState().toggleMaximize("player");
    expect(store.getState().maximized).toBe("player");
    store.getState().toggleMaximize("player");
    expect(store.getState().maximized).toBeNull();
  });

  it("ignores unknown ids and leaves layout state untouched", () => {
    const store = createShellLayoutStore();
    seed(store);
    const before = store.getState().open;
    store.getState().toggleMaximize("nope");
    expect(store.getState().maximized).toBeNull();
    expect(store.getState().open).toEqual(before);
  });
});
