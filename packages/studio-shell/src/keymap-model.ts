/**
 * The keymap editor's model — everything the Keymap settings table needs to
 * ask, kept out of the React component so it can be tested directly.
 *
 * The load-bearing rule here is the rebinding ruling (2026-08-30, decision
 * log "A rebound key displaces the old owner, and says so"). [`Keymap`]'s
 * `byChord` is a `Map<chordId, commandId>`, so two commands holding one
 * chord means the last registered silently wins and the other is dead with
 * nothing reporting it. An editor that let an author create that state
 * would be letting them configure something that does not work. So
 * {@link bindChord} takes the chord off its previous owner and reports who
 * that was, for the UI to say out loud.
 *
 * Commands keep MULTIPLE bindings throughout. Several ship two or three
 * defaults specifically to dodge browser-reserved chords (#107 — Firefox
 * eats Mod-Shift-P), and an override replaces a command's WHOLE default
 * set, so an editor that tracked one binding per command would silently
 * drop the alternates those defaults exist to provide.
 */

import {
  type Chord,
  type KeymapOverrides,
  chordId,
  parseKeybinding,
} from "./keymap.js";

/** The subset of `Command` this model reads. */
export interface KeymapCommand {
  id: string;
  title: string;
  keybinding?: string | readonly string[];
}

/** Where a command's current bindings come from. */
export type KeymapSource = "default" | "custom" | "unbound";

export interface KeymapRow {
  id: string;
  /** Full palette title, e.g. "Search: Find in files". */
  title: string;
  /** Leading "Category:" segment, or "Other" when the title has none. */
  category: string;
  /** Title with the category stripped, for display in a grouped table. */
  name: string;
  /** Effective bindings, in order; empty when unbound. */
  chords: Chord[];
  source: KeymapSource;
  /**
   * Whether the author has an override for this command.
   *
   * Distinct from `source`, and the distinction is load-bearing: a command
   * that simply ships no keybinding reads "unbound" while having nothing to
   * reset, so a view keyed on `source` alone offers it a reset button that
   * does nothing, and counts it among the author's customisations.
   */
  overridden: boolean;
}

/**
 * Reverse of {@link parseKeybinding}: a chord as a storable binding string.
 *
 * Not [`chordId`], which is the lookup spelling (`"mod+shift+p"`) and does
 * not parse back. `-` and ` ` go out as their alias names because `"Mod--"`
 * splits into an empty segment and is unparsable — the very reason
 * `KEY_ALIASES` exists. `keymap-model.test.ts` round-trips every chord the
 * capture UI can produce.
 */
export function serializeChord(chord: Chord): string {
  const parts: string[] = [];
  if (chord.mod) parts.push("Mod");
  if (chord.alt) parts.push("Alt");
  if (chord.shift) parts.push("Shift");
  parts.push(serializeKey(chord.key));
  return parts.join("-");
}

/**
 * A chord's key in the spelling the shipped defaults use.
 *
 * `chordFromEvent` lowercases, so a captured key would serialize as
 * `"Mod-k"` beside a default written `"Mod-Shift-F"`. Parsing lowercases
 * again, so this is cosmetic — but the JSON escape hatch shows both, and a
 * file that spells the same thing two ways invites the reader to think the
 * difference means something.
 */
function serializeKey(key: string): string {
  const alias = SERIALIZED_KEYS[key];
  if (alias !== undefined) return alias;
  if (key.length === 1) return key.toUpperCase();
  const fn = /^f(\d{1,2})$/.exec(key);
  if (fn !== null) return `F${fn[1]}`;
  return NAMED_KEYS[key] ?? key;
}

/** Keys that cannot be written literally in a `-`-delimited binding. */
const SERIALIZED_KEYS: Record<string, string> = {
  "-": "Minus",
  " ": "Space",
};

/** Multi-character keys, capitalised to match the defaults' house style. */
const NAMED_KEYS: Record<string, string> = {
  enter: "Enter",
  escape: "Escape",
  tab: "Tab",
  backspace: "Backspace",
  delete: "Delete",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pagedown: "PageDown",
  arrowup: "ArrowUp",
  arrowdown: "ArrowDown",
  arrowleft: "ArrowLeft",
  arrowright: "ArrowRight",
};

/** A command's default bindings, parsed; empty when it ships none. */
function defaultChords(command: KeymapCommand): Chord[] {
  return toChords(command.keybinding);
}

/** A command's effective bindings — override if it has one, else defaults. */
export function effectiveChords(
  command: KeymapCommand,
  overrides: KeymapOverrides,
): Chord[] {
  return has(overrides, command.id)
    ? toChords(overrides[command.id])
    : defaultChords(command);
}

function toChords(binding: string | readonly string[] | null | undefined): Chord[] {
  if (binding === null || binding === undefined) return [];
  const list = typeof binding === "string" ? [binding] : binding;
  const chords: Chord[] = [];
  for (const one of list) {
    // Unparsable bindings are skipped, matching `Keymap.fromCommands` — the
    // table must show what will actually resolve, not what was typed.
    const chord = parseKeybinding(one);
    if (chord !== null) chords.push(chord);
  }
  return chords;
}

const has = (o: KeymapOverrides, id: string): boolean =>
  Object.prototype.hasOwnProperty.call(o, id);

/** One row per command, sorted by category then name. */
export function keymapRows(
  commands: readonly KeymapCommand[],
  overrides: KeymapOverrides,
): KeymapRow[] {
  const rows = commands.map((command) => {
    const chords = effectiveChords(command, overrides);
    const split = command.title.indexOf(":");
    return {
      id: command.id,
      title: command.title,
      category: split === -1 ? "Other" : command.title.slice(0, split).trim(),
      name: split === -1 ? command.title : command.title.slice(split + 1).trim(),
      chords,
      source: sourceOf(command, overrides, chords),
      overridden: has(overrides, command.id),
    };
  });
  rows.sort((a, b) => a.category.localeCompare(b.category) || a.name.localeCompare(b.name));
  return rows;
}

/**
 * "custom" only when the effective set actually DIFFERS from the default.
 *
 * An override equal to the defaults is written by ordinary editing (bind a
 * chord, then bind it back) and must not leave the row reading "Custom"
 * with a reset button that changes nothing.
 */
function sourceOf(
  command: KeymapCommand,
  overrides: KeymapOverrides,
  chords: Chord[],
): KeymapSource {
  if (chords.length === 0) return "unbound";
  return sameChords(chords, defaultChords(command)) ? "default" : "custom";
}

const sameChords = (a: Chord[], b: Chord[]): boolean =>
  a.length === b.length && a.every((chord, i) => chordId(chord) === chordId(b[i]));

/** The command a chord currently resolves to, if any. */
export function chordOwner(
  commands: readonly KeymapCommand[],
  overrides: KeymapOverrides,
  chord: Chord,
): string | undefined {
  const id = chordId(chord);
  for (const command of commands) {
    if (effectiveChords(command, overrides).some((c) => chordId(c) === id)) {
      return command.id;
    }
  }
  return undefined;
}

export interface BindResult {
  overrides: KeymapOverrides;
  /** The command that lost this chord, if any — for the UI to name. */
  displaced: { id: string; title: string; nowUnbound: boolean } | null;
}

/**
 * Give `commandId` an additional `chord`, taking it off whoever held it.
 *
 * The displaced command loses ONLY the colliding chord, not its whole set:
 * a command with three defaults that collides on one keeps the other two.
 * It reports `nowUnbound` when the removal emptied it, since that is the
 * case worth wording differently in the warning.
 */
export function bindChord(
  commands: readonly KeymapCommand[],
  overrides: KeymapOverrides,
  commandId: string,
  chord: Chord,
): BindResult {
  const id = chordId(chord);
  let next: KeymapOverrides = { ...overrides };
  let displaced: BindResult["displaced"] = null;

  const owner = commands.find(
    (c) => c.id !== commandId && effectiveChords(c, next).some((x) => chordId(x) === id),
  );
  if (owner !== undefined) {
    const kept = effectiveChords(owner, next).filter((x) => chordId(x) !== id);
    next = writeChords(next, owner, kept);
    displaced = { id: owner.id, title: owner.title, nowUnbound: kept.length === 0 };
  }

  const target = commands.find((c) => c.id === commandId);
  if (target === undefined) return { overrides: next, displaced };
  const current = effectiveChords(target, next);
  if (!current.some((x) => chordId(x) === id)) {
    next = writeChords(next, target, [...current, chord]);
  }
  return { overrides: next, displaced };
}

/** Take one chord off a command. */
export function unbindChord(
  commands: readonly KeymapCommand[],
  overrides: KeymapOverrides,
  commandId: string,
  chord: Chord,
): KeymapOverrides {
  const command = commands.find((c) => c.id === commandId);
  if (command === undefined) return overrides;
  const id = chordId(chord);
  const kept = effectiveChords(command, overrides).filter((x) => chordId(x) !== id);
  return writeChords({ ...overrides }, command, kept);
}

/** Drop a command's override, returning it to its shipped defaults. */
export function resetCommand(
  overrides: KeymapOverrides,
  commandId: string,
): KeymapOverrides {
  if (!has(overrides, commandId)) return overrides;
  const next = { ...overrides };
  delete next[commandId];
  return next;
}

/**
 * Store `chords` for `command`, or drop the override when they match its
 * defaults — so round-tripping an edit leaves no residue in the file, and
 * the row goes back to reading "Default".
 *
 * An empty set is stored as `null` (explicitly unbound) rather than
 * dropped, because dropping it would restore the defaults instead.
 */
function writeChords(
  overrides: KeymapOverrides,
  command: KeymapCommand,
  chords: Chord[],
): KeymapOverrides {
  const next = { ...overrides };
  if (sameChords(chords, defaultChords(command))) {
    delete next[command.id];
    return next;
  }
  next[command.id] = chords.length === 0 ? null : chords.map(serializeChord);
  return next;
}
