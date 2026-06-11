/**
 * @brink/studio-shell — maximize commands (docs/studio-shell-spec.md §5.4).
 *
 * Two maximize modes share one section of the spec:
 *
 * - `view.maximize` (tool windows): the tool window covers the whole shell
 *   frame; the editor unmounts (layout-store `maximized`).
 * - `editor.maximizeGroup` (editor groups): the focused (or given) group
 *   takes the entire editor area — other groups hide and the open docks
 *   collapse (editor-groups `maximizedGroupId`). The editor itself never
 *   unmounts. Replaces the retired player-specific fullscreen.
 *
 * Interplay rule: the two modes are mutually exclusive. Dispatching either
 * command while the other mode is active restores the other first, so at
 * most one maximize is ever in effect. `Escape` restores whichever is
 * active (ShellFrame). Registered together because the exclusivity couples
 * them — neither store knows about the other.
 */

import type { CommandRegistry } from "./command.js";
import type { EditorGroupsStore } from "./editor-groups.js";
import type { ShellLayoutStore } from "./layout-store.js";

export const VIEW_MAXIMIZE_COMMAND_ID = "view.maximize";
export const EDITOR_MAXIMIZE_GROUP_COMMAND_ID = "editor.maximizeGroup";

/**
 * Register `view.maximize` (args: tool-window id) and `editor.maximizeGroup`
 * (args: optional group id, defaulting to the focused group; both unbound —
 * palette-discoverable). Returns a disposer that unregisters both.
 */
export function registerMaximizeCommands(
  commands: CommandRegistry,
  layout: ShellLayoutStore,
  groups: EditorGroupsStore,
): () => void {
  const disposers = [
    commands.register({
      id: VIEW_MAXIMIZE_COMMAND_ID,
      title: "View: Toggle Maximized Tool Window",
      run: (args) => {
        if (typeof args !== "string") return;
        // Mutual exclusion: an active group maximize restores first.
        if (groups.getState().maximizedGroupId !== null) {
          groups.getState().toggleMaximizeGroup();
        }
        layout.getState().toggleMaximize(args);
      },
    }),

    commands.register({
      id: EDITOR_MAXIMIZE_GROUP_COMMAND_ID,
      title: "Editor: Toggle Maximized Group",
      run: (args) => {
        // Mutual exclusion: an active tool-window maximize restores first.
        const maximized = layout.getState().maximized;
        if (maximized !== null) layout.getState().toggleMaximize(maximized);
        groups
          .getState()
          .toggleMaximizeGroup(typeof args === "string" ? args : undefined);
      },
    }),
  ];

  return () => {
    for (const dispose of disposers) dispose();
  };
}
