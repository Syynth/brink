/**
 * Player document tests (issue #120, spec §4 / §7.6 / §7.8).
 *
 * Covers: the story.openPlayer command opening/focusing the singleton tab,
 * the default-layout bootstrap helper (Inky two-up: player split right,
 * focus back on the editor group), split-duplicating the player tab (two
 * views over one session document), and the component contract — session
 * placeholder with a Start affordance, command-only interactions (the
 * provider-agnostic rule: session data in, command dispatches out, never a
 * runner handle).
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
