/**
 * @brink/studio-shell — editor-group commands (spec §7.8, §6).
 *
 * The keyboard/palette surface over the editor-groups store, registered by
 * ShellProvider like the generated view-toggle commands: `editor.split`
 * (Mod-\, VS Code's split chord), `editor.moveTabRight` / `editor.moveTabLeft`
 * (move the focused group's active tab between neighbor groups), and
 * `editor.focusNextGroup` (unbound; palette-discoverable).
 */

import type { CommandRegistry } from "./command.js";
import {
  focusedGroup,
  type EditorGroupsState,
  type EditorGroupsStore,
} from "./editor-groups.js";

function groupIndex(state: EditorGroupsState): number {
  return state.groups.findIndex((g) => g.id === state.focusedGroupId);
}

/**
 * Register the editor-group commands against `groups`. Returns a disposer
 * that unregisters them all.
 */
export function registerEditorGroupCommands(
  commands: CommandRegistry,
  groups: EditorGroupsStore,
): () => void {
  const disposers = [
    commands.register({
      id: "editor.split",
      title: "Editor: Split",
      keybinding: "Mod-\\",
      run: () => groups.getState().splitGroup(),
    }),

    commands.register({
      id: "editor.moveTabRight",
      title: "Editor: Move Tab to Right Group",
      when: () => {
        const s = groups.getState();
        const g = focusedGroup(s);
        if (g.activeKey === null) return false;
        // A right group must exist, or there must be something left behind
        // to split away from (moving a group's only tab right would just
        // re-create the same layout one slot over).
        return groupIndex(s) < s.groups.length - 1 || g.tabs.length > 1;
      },
      run: () => {
        const s = groups.getState();
        const g = focusedGroup(s);
        if (g.activeKey === null) return;
        const right = s.groups[groupIndex(s) + 1];
        if (right) {
          s.moveTabToGroup(g.activeKey, g.id, right.id);
        } else if (g.tabs.length > 1) {
          // No right neighbor: split (duplicate into a new right group),
          // then drop the source copy — a move, built from primitives.
          s.splitGroup();
          groups.getState().closeTab(g.id, g.activeKey);
        }
      },
    }),

    commands.register({
      id: "editor.moveTabLeft",
      title: "Editor: Move Tab to Left Group",
      when: () => {
        const s = groups.getState();
        return groupIndex(s) > 0 && focusedGroup(s).activeKey !== null;
      },
      run: () => {
        const s = groups.getState();
        const g = focusedGroup(s);
        const left = s.groups[groupIndex(s) - 1];
        if (g.activeKey !== null && left) {
          s.moveTabToGroup(g.activeKey, g.id, left.id);
        }
      },
    }),

    commands.register({
      id: "editor.focusNextGroup",
      title: "Editor: Focus Next Group",
      when: () => groups.getState().groups.length > 1,
      run: () => {
        const s = groups.getState();
        const next = s.groups[(groupIndex(s) + 1) % s.groups.length];
        s.focusGroup(next.id);
      },
    }),
  ];

  return () => {
    for (const dispose of disposers) dispose();
  };
}
