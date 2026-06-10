/**
 * @brink/studio-shell — command palette (docs/studio-shell-spec.md §6).
 *
 * A QuickPick over enabled commands with effective (post-override)
 * keybindings as details. Registers its own `palette.toggle` command on
 * mount — the keymap rebuilds via the registry's change event, so the
 * bindings work without bootstrap-order coupling.
 */

import { useCallback, useEffect, useState } from "react";
import type { Command } from "./command.js";
import { formatChord } from "./keymap.js";
import { QuickPick, rankQuickPickItems, type QuickPickItem } from "./quickpick.js";
import { useShell } from "./shell-context.js";

export const PALETTE_COMMAND_ID = "palette.toggle";

/**
 * Enabled commands matching `query` (title preferred over id), ranked like
 * the picker ranks. Kept for tests and non-React callers.
 */
export function filterCommands(commands: readonly Command[], query: string): Command[] {
  const enabled = commands.filter((c) => c.when === undefined || c.when());
  return rankQuickPickItems(
    enabled.map((command) => ({
      key: command.id,
      title: command.title,
      searchText: command.id,
      command,
    })),
    query,
  ).map((item) => item.command);
}

interface CommandItem extends QuickPickItem {
  commandId: string;
}

export function CommandPalette() {
  const { commands, keymap, isMac } = useShell();
  const [open, setOpen] = useState(false);

  useEffect(
    () =>
      commands.register({
        id: PALETTE_COMMAND_ID,
        title: "Command Palette",
        // Firefox reserves Mod-Shift-P (private browsing) and never delivers
        // it to content (#107) — Mod-Shift-L is the cross-browser alternate;
        // F1 works thanks to the keyhandler's function-key exemption.
        keybinding: ["Mod-Shift-P", "Mod-Shift-L", "F1"],
        run: () => setOpen((wasOpen) => !wasOpen),
      }),
    [commands],
  );

  const close = useCallback(() => setOpen(false), []);

  const items: CommandItem[] = open
    ? commands
        .list()
        .filter((c) => c.id !== PALETTE_COMMAND_ID && (c.when === undefined || c.when()))
        .map((command) => {
          const chord = keymap.bindingFor(command.id);
          return {
            key: command.id,
            title: command.title,
            detail: chord ? formatChord(chord, isMac) : undefined,
            searchText: command.id,
            commandId: command.id,
          };
        })
    : [];

  return (
    <QuickPick
      open={open}
      onClose={close}
      items={items}
      onPick={(item) => commands.dispatch(item.commandId)}
      placeholder="Run a command…"
      emptyText="No matching commands"
      ariaLabel="Command palette"
    />
  );
}
