/**
 * @brink/studio-shell — command palette (docs/studio-shell-spec.md §6).
 *
 * A centered Overlay listing enabled commands, fuzzy-filtered, with effective
 * (post-override) keybindings shown. Registers its own `palette.toggle`
 * command on mount — the keymap rebuilds via the registry's change event, so
 * Mod-Shift-P works without bootstrap-order coupling.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { Command } from "./command.js";
import { formatChord } from "./keymap.js";
import { Overlay } from "./overlay.js";
import { useShell } from "./shell-context.js";

export const PALETTE_COMMAND_ID = "palette.toggle";

/**
 * Enabled commands whose title (or id) matches `query` as a case-insensitive
 * subsequence, ranked: earlier and more compact matches first, ties by
 * registration order. Exported for tests.
 */
export function filterCommands(commands: readonly Command[], query: string): Command[] {
  const enabled = commands.filter((c) => c.when === undefined || c.when());
  const q = query.trim().toLowerCase();
  if (q === "") return enabled;

  const scored: { command: Command; score: number }[] = [];
  for (const command of enabled) {
    const score = Math.min(
      subsequenceScore(command.title.toLowerCase(), q),
      subsequenceScore(command.id.toLowerCase(), q) + 1, // prefer title matches
    );
    if (score !== Number.POSITIVE_INFINITY) scored.push({ command, score });
  }
  scored.sort((a, b) => a.score - b.score);
  return scored.map((s) => s.command);
}

/** Lower is better; Infinity if `query` is not a subsequence of `text`. */
function subsequenceScore(text: string, query: string): number {
  let pos = text.indexOf(query[0] ?? "");
  if (pos === -1) return Number.POSITIVE_INFINITY;
  const start = pos;
  for (let i = 1; i < query.length; i++) {
    pos = text.indexOf(query.charAt(i), pos + 1);
    if (pos === -1) return Number.POSITIVE_INFINITY;
  }
  // Contiguity dominates, then earliness.
  return (pos - start - (query.length - 1)) * 100 + start;
}

export function CommandPalette() {
  const { commands, keymap, isMac } = useShell();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(
    () =>
      commands.register({
        id: PALETTE_COMMAND_ID,
        title: "Command Palette",
        // Firefox reserves Mod-Shift-P (private browsing) and never delivers
        // it to content (#107) — Mod-Shift-L is the cross-browser alternate;
        // F1 works thanks to the keyhandler's function-key exemption.
        keybinding: ["Mod-Shift-P", "Mod-Shift-L", "F1"],
        run: () => {
          setOpen((wasOpen) => !wasOpen);
          setQuery("");
          setSelected(0);
        },
      }),
    [commands],
  );

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  const close = useCallback(() => setOpen(false), []);

  const items = open
    ? filterCommands(
        commands.list().filter((c) => c.id !== PALETTE_COMMAND_ID),
        query,
      )
    : [];
  const clampedSelected = Math.min(selected, Math.max(0, items.length - 1));

  const runItem = (command: Command): void => {
    // Close first so focus returns before the command's effects land.
    close();
    commands.dispatch(command.id);
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLInputElement>): void => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (items.length === 0) return;
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setSelected((items.length + clampedSelected + delta) % items.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const item = items[clampedSelected];
      if (item) runItem(item);
    }
  };

  return (
    <Overlay open={open} onClose={close} className="shell-palette">
      <input
        ref={inputRef}
        className="shell-palette-input"
        type="text"
        placeholder="Run a command…"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setSelected(0);
        }}
        onKeyDown={onKeyDown}
        aria-label="Command palette"
      />
      <ul className="shell-palette-list" role="listbox">
        {items.map((command, index) => {
          const chord = keymap.bindingFor(command.id);
          return (
            <li
              key={command.id}
              role="option"
              aria-selected={index === clampedSelected}
              className={
                "shell-palette-item" + (index === clampedSelected ? " selected" : "")
              }
              onMouseEnter={() => setSelected(index)}
              onClick={() => runItem(command)}
            >
              <span className="title">{command.title}</span>
              {chord && <span className="binding">{formatChord(chord, isMac)}</span>}
            </li>
          );
        })}
        {items.length === 0 && <li className="shell-palette-empty">No matching commands</li>}
      </ul>
    </Overlay>
  );
}
