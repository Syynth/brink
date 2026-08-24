/**
 * @brink/studio-shell — hamburger menu (docs/studio-shell-spec.md §6).
 *
 * A single icon at the top of the left strip (JetBrains new-UI placement)
 * opening a grouped menu *generated from the command registry* — no
 * hand-maintained menu structure, embed-friendly, and automatically complete
 * like the palette. Groups come from command id prefixes ("story.restart" →
 * "Story"), in first-appearance (registration) order.
 */

import { useState } from "react";
import type { Command } from "./command.js";
import { formatChord } from "./keymap.js";
import { Overlay } from "./overlay.js";
import { useShell } from "./shell-context.js";

export interface MenuGroup {
  /** Display label derived from the id prefix, e.g. "Quick Open". */
  label: string;
  commands: Command[];
}

/**
 * Group enabled commands by id prefix, groups in first-appearance order,
 * commands in registration order within. Pure and exported for tests.
 */
export function groupCommandsForMenu(commands: readonly Command[]): MenuGroup[] {
  const groups = new Map<string, MenuGroup>();
  for (const command of commands) {
    if (command.when !== undefined && !command.when()) continue;
    const prefix = command.id.split(".")[0] ?? command.id;
    let group = groups.get(prefix);
    if (group === undefined) {
      group = { label: prefixLabel(prefix), commands: [] };
      groups.set(prefix, group);
    }
    group.commands.push(command);
  }
  return [...groups.values()];
}

/** "quickOpen" → "Quick Open", "story" → "Story". */
function prefixLabel(prefix: string): string {
  const spaced = prefix.replace(/([a-z0-9])([A-Z])/g, "$1 $2");
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

export function HamburgerMenu() {
  const { commands, keymap, isMac } = useShell();
  const [open, setOpen] = useState(false);
  const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null);

  const groups = open ? groupCommandsForMenu(commands.list()) : [];

  const runItem = (id: string): void => {
    setOpen(false);
    commands.dispatch(id);
  };

  return (
    <>
      <button
        ref={setAnchor}
        type="button"
        className={"shell-strip-btn shell-hamburger" + (open ? " active" : "")}
        title="Menu"
        aria-label="Menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <svg
          viewBox="0 0 16 16"
          width="16"
          height="16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          aria-hidden="true"
        >
          <path d="M3 4.5h10M3 8h10M3 11.5h10" />
        </svg>
      </button>
      <Overlay
        open={open}
        onClose={() => setOpen(false)}
        anchor={anchor}
        placement="right-start"
        className="shell-menu"
      >
        {groups.map((group) => (
          <div key={group.label} className="shell-menu-group">
            <div className="shell-menu-group-label">{group.label}</div>
            {group.commands.map((command) => {
              const chord = keymap.bindingFor(command.id);
              return (
                <button
                  key={command.id}
                  type="button"
                  className="shell-menu-item"
                  onClick={() => runItem(command.id)}
                >
                  <span className="title">{command.title}</span>
                  {chord && <span className="binding">{formatChord(chord, isMac)}</span>}
                </button>
              );
            })}
          </div>
        ))}
      </Overlay>
    </>
  );
}
