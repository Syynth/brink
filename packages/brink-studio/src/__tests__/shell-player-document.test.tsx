/**
 * Player document tests (issue #120, spec §4 / §7.6 / §7.8).
 *
 * Covers: the story.openPlayer command opening/focusing the singleton tab,
 * the default-layout bootstrap helper (Inky two-up: player split right,
 * focus back on the editor group), split-duplicating the player tab (two
 * views over one session document), the component contract — session
 * placeholder with a Start affordance, command-only interactions (the
 * provider-agnostic rule: session data in, command dispatches out, never a
 * runner handle) — and #280: reopening the player after it was closed
 * restores the two-up split instead of dropping the tab into the focused
 * group, and the hamburger menu (landed #2690, after this issue was filed)
 * already gives a second, mouse-discoverable route to `story.openPlayer`
 * beyond the command palette.
 */

import { describe, expect, it, afterEach } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  ShellProvider,
  createEditorGroupsStore,
  documentKey,
  findTab,
  focusedGroup,
  groupCommandsForMenu,
} from "@brink/studio-shell";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import {
  OPEN_PLAYER_COMMAND_ID,
  PLAYER_TYPE_ID,
  PlayerPane,
  StoreProvider,
  openPlayerSplit,
  playerRef,
  registerOpenPlayerCommand,
} from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

// ── Command wiring ──────────────────────────────────────────────────

describe("story.openPlayer", () => {
  it("opens the singleton pinned in the focused group", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerOpenPlayerCommand(commands, groups);

    expect(commands.dispatch(OPEN_PLAYER_COMMAND_ID)).toBe(true);

    const key = documentKey(playerRef());
    const found = findTab(groups.getState().groups, key);
    expect(found).not.toBeNull();
    expect(found!.tab.pinned).toBe(true);
    expect(found!.tab.ref.typeId).toBe(PLAYER_TYPE_ID);
    expect(found!.tab.ref.title).toBe("Player");
    expect(found!.group.activeKey).toBe(key);
    // A genuinely empty single group (nothing open yet — not #280's
    // "the split collapsed" case) opens in place; it does not force a split.
    expect(groups.getState().groups).toHaveLength(1);
  });

  it("re-dispatch focuses the existing tab instead of duplicating", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerOpenPlayerCommand(commands, groups);

    // Player in group 1; focus moves to a right split holding an ink file.
    commands.dispatch(OPEN_PLAYER_COMMAND_ID);
    const homeGroupId = groups.getState().focusedGroupId;
    groups.getState().openDocument(
      { typeId: "ink-file", docId: "main.ink", title: "main.ink" },
      { group: "split-right" },
    );
    expect(groups.getState().focusedGroupId).not.toBe(homeGroupId);

    commands.dispatch(OPEN_PLAYER_COMMAND_ID);

    const s = groups.getState();
    expect(s.focusedGroupId).toBe(homeGroupId);
    const key = documentKey(playerRef());
    const copies = s.groups.flatMap((g) =>
      g.tabs.filter((t) => documentKey(t.ref) === key),
    );
    expect(copies).toHaveLength(1);
  });

  // #280: closing the player used to feel permanent — the only route back
  // (the palette/hamburger command) dropped the tab into whichever group
  // happened to be focused instead of restoring the two-up split a fresh
  // load gives you, and there is no drag-to-split to repair it by hand.
  it("reopening after the player tab was closed restores the two-up split", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerOpenPlayerCommand(commands, groups);

    // Bootstrap, as mount.tsx does: entry file in group 1, player split
    // right via openPlayerSplit (not the command), focus back on the entry.
    groups
      .getState()
      .openDocument({ typeId: "ink-file", docId: "main.ink", title: "main.ink" });
    const entryGroupId = groups.getState().focusedGroupId;
    openPlayerSplit(groups);
    expect(groups.getState().groups).toHaveLength(2);
    expect(groups.getState().focusedGroupId).toBe(entryGroupId);

    // User closes the player tab — its group collapses (last tab gone),
    // leaving a single group again, same shape as a fresh unsplit load.
    const playerGroupId = groups
      .getState()
      .groups.find((g) => g.id !== entryGroupId)!.id;
    groups.getState().closeTab(playerGroupId, documentKey(playerRef()));
    expect(groups.getState().groups).toHaveLength(1);
    expect(groups.getState().groups[0].id).toBe(entryGroupId);

    // Reopen via the command (palette or hamburger menu) — not manually.
    commands.dispatch(OPEN_PLAYER_COMMAND_ID);

    const s = groups.getState();
    expect(s.groups).toHaveLength(2);
    expect(s.groups[0].id).toBe(entryGroupId);
    expect(s.groups[0].tabs.map((t) => t.ref.typeId)).toEqual(["ink-file"]);
    expect(s.groups[1].tabs.map((t) => t.ref.typeId)).toEqual([PLAYER_TYPE_ID]);
    expect(s.groups[1].activeKey).toBe(documentKey(playerRef()));
    // Focus hands back to the editor, exactly like the fresh-load bootstrap.
    expect(s.focusedGroupId).toBe(entryGroupId);
  });

  // The maximized-single-group case (review finding on #2787): closing the
  // player collapses to one group, same as above, but this time the user
  // maximized that lone group before reopening. openDocument's split-right
  // branch must clear maximizedGroupId itself (mirroring splitGroup, §5.4) —
  // otherwise the new player group is created behind the maximized group and
  // EditorArea (which renders only the maximized group) never shows it.
  it("clears maximizedGroupId when restoring the split from a maximized single group", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerOpenPlayerCommand(commands, groups);

    groups
      .getState()
      .openDocument({ typeId: "ink-file", docId: "main.ink", title: "main.ink" });
    const entryGroupId = groups.getState().focusedGroupId;
    openPlayerSplit(groups);
    const playerGroupId = groups
      .getState()
      .groups.find((g) => g.id !== entryGroupId)!.id;
    groups.getState().closeTab(playerGroupId, documentKey(playerRef()));
    expect(groups.getState().groups).toHaveLength(1);

    // Maximize the sole remaining group before reopening the player.
    groups.getState().toggleMaximizeGroup(entryGroupId);
    expect(groups.getState().maximizedGroupId).toBe(entryGroupId);

    commands.dispatch(OPEN_PLAYER_COMMAND_ID);

    const s = groups.getState();
    expect(s.groups).toHaveLength(2);
    expect(s.maximizedGroupId).toBeNull();
    expect(s.groups[1].tabs.map((t) => t.ref.typeId)).toEqual([PLAYER_TYPE_ID]);
  });

  // Once the editor area is already split beyond the two-up, there is no
  // "missing" layout to restore — reopening should not keep stacking new
  // columns, so it falls back to the normal reveal/open-in-focused policy.
  it("does not force another split when multiple groups already exist", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerOpenPlayerCommand(commands, groups);

    groups
      .getState()
      .openDocument({ typeId: "ink-file", docId: "main.ink", title: "main.ink" });
    groups.getState().openDocument(
      { typeId: "ink-file", docId: "other.ink", title: "other.ink" },
      { group: "split-right" },
    );
    const focusedBefore = groups.getState().focusedGroupId;
    expect(groups.getState().groups).toHaveLength(2);

    commands.dispatch(OPEN_PLAYER_COMMAND_ID);

    const s = groups.getState();
    expect(s.groups).toHaveLength(2);
    expect(s.focusedGroupId).toBe(focusedBefore);
    const found = findTab(s.groups, documentKey(playerRef()));
    expect(found).not.toBeNull();
    expect(found!.group.id).toBe(focusedBefore);
  });
});

// ── Discoverability (#280 premise check) ─────────────────────────────
//
// The issue's other half — "the only way back is the undiscoverable palette
// command" — predates the hamburger menu (#2684/#2690, merged the day this
// issue was re-dispatched). registerOpenPlayerCommand sets no `when` gate,
// so groupCommandsForMenu (what HamburgerMenu renders) always lists it under
// "Story" — closed or open — giving a second, mouse-discoverable route.

describe("hamburger menu surfaces story.openPlayer (#280 premise check)", () => {
  it("lists 'Story: Open Player' under the Story group even while closed", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerOpenPlayerCommand(commands, groups);

    // Nothing opened yet — the player tab is closed/never-opened.
    const menuGroups = groupCommandsForMenu(commands.list());
    const story = menuGroups.find((g) => g.label === "Story");
    expect(story).toBeDefined();
    expect(story!.commands.map((c) => c.id)).toContain(OPEN_PLAYER_COMMAND_ID);
    expect(
      story!.commands.find((c) => c.id === OPEN_PLAYER_COMMAND_ID)!.title,
    ).toBe("Story: Open Player");
  });
});

// ── Default layout (Inky two-up) ────────────────────────────────────

describe("openPlayerSplit (bootstrap default layout)", () => {
  it("opens the player in a right split and hands focus back to the editor", () => {
    const groups = createEditorGroupsStore();
    // The entry file is already open in the first group (bootstrap order).
    groups
      .getState()
      .openDocument({ typeId: "ink-file", docId: "main.ink", title: "main.ink" });
    const entryGroupId = groups.getState().focusedGroupId;

    openPlayerSplit(groups);

    const s = groups.getState();
    expect(s.groups).toHaveLength(2);
    // Editor left, player right.
    expect(s.groups[0].id).toBe(entryGroupId);
    expect(s.groups[1].tabs.map((t) => t.ref.typeId)).toEqual([PLAYER_TYPE_ID]);
    expect(s.groups[1].activeKey).toBe(documentKey(playerRef()));
    // Typing keeps going to the editor.
    expect(s.focusedGroupId).toBe(entryGroupId);
  });
});

// ── Split duplicates the player (VS Code semantics, §7.8) ───────────

describe("splitting a group whose active tab is the player", () => {
  it("duplicates the tab — two views of the one session document", () => {
    const commands = new CommandRegistry();
    const groups = createEditorGroupsStore();
    registerOpenPlayerCommand(commands, groups);
    commands.dispatch(OPEN_PLAYER_COMMAND_ID);

    groups.getState().splitGroup();

    const s = groups.getState();
    const key = documentKey(playerRef());
    expect(s.groups).toHaveLength(2);
    for (const g of s.groups) {
      expect(g.tabs.map((t) => documentKey(t.ref))).toEqual([key]);
    }
    expect(focusedGroup(s).id).toBe(s.groups[1].id);
  });
});

// ── Component contract ──────────────────────────────────────────────

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  if (root) act(() => root!.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(commands: CommandRegistry, store: StudioStore, ui: ReactNode) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        { commands, children: null } as never,
        createElement(StoreProvider, { store, children: ui } as never),
      ),
    );
  });
  return container;
}

function playerView(groupId: string) {
  return createElement(PlayerPane, {
    doc: playerRef(),
    groupId,
    active: true,
  });
}

describe("PlayerPane as a document view", () => {
  it("renders the session placeholder and Start dispatches story.start", () => {
    const commands = new CommandRegistry();
    const dispatched: string[] = [];
    commands.register({
      id: "story.start",
      title: "Story: Start",
      run: () => void dispatched.push("story.start"),
    });
    const store = createStudioStore();

    const el = mount(commands, store, playerView("group-1"));

    expect(el.textContent).toContain("No story session");
    const start = el.querySelector<HTMLButtonElement>(".session-placeholder-start");
    expect(start).not.toBeNull();
    act(() => start!.click());
    expect(dispatched).toEqual(["story.start"]);
  });

  it("two instances render the same session; choices dispatch story.choose", () => {
    const commands = new CommandRegistry();
    const chosen: unknown[] = [];
    commands.register({
      id: "story.choose",
      title: "Story: Choose",
      run: (args) => void chosen.push(args),
    });
    const store = createStudioStore();
    store.setState({
      sessionStatus: "awaiting-choice",
      sessionText: ["The lights dim."],
      sessionLines: [{ text: "The lights dim.", kind: "line" as const, tags: [] }],
      sessionChoices: [{ index: 0, text: "Step forward", tags: [] }],
    } as never);

    // The split-player case: two views (groups) over one session document.
    const el = mount(
      commands,
      store,
      createElement(
        "div",
        null,
        playerView("group-1"),
        playerView("group-2"),
      ),
    );

    const panes = el.querySelectorAll(".player-pane");
    expect(panes).toHaveLength(2);
    for (const pane of panes) {
      expect(pane.textContent).toContain("The lights dim.");
      expect(pane.textContent).toContain("Step forward");
    }

    // Both views drive the one session through commands only.
    const buttons = el.querySelectorAll<HTMLButtonElement>(".choices button");
    expect(buttons).toHaveLength(2);
    act(() => buttons[1].click());
    expect(chosen).toEqual([0]);
  });
});
