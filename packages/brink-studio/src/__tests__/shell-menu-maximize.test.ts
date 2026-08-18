/**
 * Hamburger menu grouping + maximize (shell issue 2.5 and #120, spec §6,
 * §5.4): tool-window maximize, editor-group maximize, and the mutual
 * exclusion between them.
 */

import { describe, expect, it } from "vitest";
import {
  CommandRegistry,
  createEditorGroupsStore,
  createShellLayoutStore,
  documentKey,
  groupCommandsForMenu,
  registerMaximizeCommands,
  type Command,
  type DocumentRef,
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

  it("syncFromRegistry clears a maximized id the registry no longer knows", () => {
    const store = createShellLayoutStore();
    seed(store);
    store.getState().toggleMaximize("player");
    // The window is deregistered (e.g. a stale persisted snapshot): the
    // maximize must not survive as a ghost id.
    store.getState().syncFromRegistry([]);
    expect(store.getState().maximized).toBeNull();
  });
});

// ── Editor-group maximize (#120, spec §5.4) ─────────────────────────

function ref(docId: string): DocumentRef {
  return { typeId: "test", docId, title: docId };
}

/** A two-group store: a.ink in group-1, b.ink split into a second group. */
function twoGroups() {
  const store = createEditorGroupsStore();
  store.getState().openDocument(ref("a.ink"));
  store.getState().openDocument(ref("b.ink"), { group: "split-right" });
  return store;
}

describe("toggleMaximizeGroup", () => {
  it("maximizes the focused group by default and restores on re-toggle", () => {
    const store = twoGroups();
    const focused = store.getState().focusedGroupId;
    store.getState().toggleMaximizeGroup();
    expect(store.getState().maximizedGroupId).toBe(focused);
    store.getState().toggleMaximizeGroup();
    expect(store.getState().maximizedGroupId).toBeNull();
  });

  it("maximizing an explicit group also focuses it", () => {
    const store = twoGroups();
    const first = store.getState().groups[0].id;
    expect(store.getState().focusedGroupId).not.toBe(first);
    store.getState().toggleMaximizeGroup(first);
    expect(store.getState().maximizedGroupId).toBe(first);
    expect(store.getState().focusedGroupId).toBe(first);
  });

  it("ignores unknown group ids", () => {
    const store = twoGroups();
    store.getState().toggleMaximizeGroup("group-99");
    expect(store.getState().maximizedGroupId).toBeNull();
  });

  it("restore touches nothing else — group sizes and tabs survive", () => {
    const store = twoGroups();
    store.getState().setGroupSize(store.getState().groups[0].id, 333);
    const before = store.getState();
    store.getState().toggleMaximizeGroup();
    store.getState().toggleMaximizeGroup();
    const after = store.getState();
    expect(after.groups).toEqual(before.groups);
    expect(after.groupSizes).toEqual(before.groupSizes);
    expect(after.focusedGroupId).toBe(before.focusedGroupId);
  });

  it("clears when the maximized group collapses (last tab closed)", () => {
    const store = twoGroups();
    const second = store.getState().focusedGroupId;
    store.getState().toggleMaximizeGroup(second);
    store.getState().closeTab(second, documentKey(ref("b.ink")));
    expect(store.getState().groups).toHaveLength(1);
    expect(store.getState().maximizedGroupId).toBeNull();
  });

  it("splitting while maximized restores first (the new group must show)", () => {
    const store = twoGroups();
    store.getState().toggleMaximizeGroup();
    store.getState().splitGroup();
    expect(store.getState().groups).toHaveLength(3);
    expect(store.getState().maximizedGroupId).toBeNull();
  });

  // #2797: a Binder click (openDocument's default "focused" target) reveals
  // an already-open tab wherever it lives. If that tab's group is hidden
  // behind a maximized sibling, EditorArea renders only the maximized group
  // (§5.4) — the reveal moved focus internally but nothing painted, so the
  // click appeared to do nothing. Mirrors the "split-right" fix from #2787.
  it("revealing a tab hidden behind a maximized sibling restores first", () => {
    const store = twoGroups();
    const first = store.getState().groups[0].id;
    const second = store.getState().groups[1].id;
    // b.ink lives in `second`; maximize `first` so `second` is unrendered.
    store.getState().toggleMaximizeGroup(first);
    expect(store.getState().maximizedGroupId).toBe(first);

    store.getState().openDocument(ref("b.ink"));

    expect(store.getState().maximizedGroupId).toBeNull();
    expect(store.getState().focusedGroupId).toBe(second);
    expect(store.getState().groups).toHaveLength(2);
  });

  // Revealing a tab that already lives in the maximized group itself needs
  // no un-maximize — it is already the only thing rendered.
  it("revealing a tab already in the maximized group leaves it maximized", () => {
    const store = twoGroups();
    const first = store.getState().groups[0].id;
    store.getState().toggleMaximizeGroup(first);

    store.getState().openDocument(ref("a.ink"));

    expect(store.getState().maximizedGroupId).toBe(first);
    expect(store.getState().focusedGroupId).toBe(first);
  });
});

describe("registerMaximizeCommands interplay", () => {
  function harness() {
    const commands = new CommandRegistry();
    const layout = createShellLayoutStore();
    layout.getState().syncFromRegistry([
      {
        id: "binder",
        title: "Binder",
        icon: null,
        defaultPlacement: { dock: "left", section: "start" },
        defaultOpen: true,
        component: () => null,
      },
    ]);
    const groups = twoGroups();
    const dispose = registerMaximizeCommands(commands, layout, groups);
    return { commands, layout, groups, dispose };
  }

  it("editor.maximizeGroup toggles the groups store (focused group, then arg)", () => {
    const { commands, groups } = harness();
    expect(commands.dispatch("editor.maximizeGroup")).toBe(true);
    expect(groups.getState().maximizedGroupId).toBe(groups.getState().focusedGroupId);
    commands.dispatch("editor.maximizeGroup");
    expect(groups.getState().maximizedGroupId).toBeNull();

    const first = groups.getState().groups[0].id;
    commands.dispatch("editor.maximizeGroup", first);
    expect(groups.getState().maximizedGroupId).toBe(first);
  });

  it("view.maximize while a group is maximized restores the group first", () => {
    const { commands, layout, groups } = harness();
    commands.dispatch("editor.maximizeGroup");
    expect(groups.getState().maximizedGroupId).not.toBeNull();

    commands.dispatch("view.maximize", "binder");
    expect(groups.getState().maximizedGroupId).toBeNull();
    expect(layout.getState().maximized).toBe("binder");
  });

  it("editor.maximizeGroup while a tool window is maximized restores it first", () => {
    const { commands, layout, groups } = harness();
    commands.dispatch("view.maximize", "binder");
    expect(layout.getState().maximized).toBe("binder");

    commands.dispatch("editor.maximizeGroup");
    expect(layout.getState().maximized).toBeNull();
    expect(groups.getState().maximizedGroupId).not.toBeNull();
  });

  it("the disposer unregisters both commands", () => {
    const { commands, dispose } = harness();
    dispose();
    expect(commands.dispatch("view.maximize", "binder")).toBe(false);
    expect(commands.dispatch("editor.maximizeGroup")).toBe(false);
  });
});
