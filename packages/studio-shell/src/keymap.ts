/**
 * @brink/studio-shell — keymap layer.
 *
 * The global key handler never reads `command.keybinding` directly: it
 * resolves through a keymap table built from registry defaults merged with a
 * user-override JSON (docs/studio-shell-spec.md §6). Overrides map command id
 * → keybinding string, or null to unbind; the loader is lenient — a malformed
 * payload yields no overrides rather than an error.
 */

export interface Chord {
  /** Normalized KeyboardEvent.key (lowercased). */
  key: string;
  /** Platform primary modifier: Cmd on macOS, Ctrl elsewhere. */
  mod: boolean;
  shift: boolean;
  alt: boolean;
}

/** command id → keybinding, or null to unbind the default. */
export type KeymapOverrides = Record<string, string | null>;

export const KEYMAP_STORAGE_KEY = "brink-studio.keymap.v1";

const MODIFIER_KEYS = new Set(["meta", "control", "shift", "alt"]);

/** Parse "Mod-Shift-P" → chord. Returns null for malformed bindings. */
export function parseKeybinding(binding: string): Chord | null {
  const chord: Chord = { key: "", mod: false, shift: false, alt: false };
  for (const part of binding.split("-")) {
    const lower = part.toLowerCase();
    if (lower === "mod") chord.mod = true;
    else if (lower === "shift") chord.shift = true;
    else if (lower === "alt") chord.alt = true;
    else if (lower !== "" && chord.key === "") chord.key = lower;
    else return null; // empty segment or a second non-modifier key
  }
  return chord.key === "" ? null : chord;
}

/** Canonical lookup id for a chord, e.g. "mod+shift+p". */
export function chordId(chord: Chord): string {
  return (
    (chord.mod ? "mod+" : "") +
    (chord.alt ? "alt+" : "") +
    (chord.shift ? "shift+" : "") +
    chord.key
  );
}

/** Chord for a keydown event; null for bare modifier presses. */
export function chordFromEvent(event: KeyboardEvent, isMac: boolean): Chord | null {
  if (MODIFIER_KEYS.has(event.key.toLowerCase())) return null;
  return {
    key: event.key.toLowerCase(),
    mod: isMac ? event.metaKey : event.ctrlKey,
    shift: event.shiftKey,
    alt: event.altKey,
  };
}

export class Keymap {
  private readonly byChord = new Map<string, string>();

  /**
   * Build the resolution table: command defaults, with overrides winning.
   * An override for an unknown command id is ignored; an unparsable binding
   * leaves the command unbound (lenient, like the loader).
   */
  static fromCommands(
    commands: readonly { id: string; keybinding?: string }[],
    overrides: KeymapOverrides = {},
  ): Keymap {
    const keymap = new Keymap();
    for (const command of commands) {
      const binding = Object.prototype.hasOwnProperty.call(overrides, command.id)
        ? overrides[command.id]
        : command.keybinding;
      if (binding === null || binding === undefined) continue;
      const chord = parseKeybinding(binding);
      if (chord !== null) keymap.byChord.set(chordId(chord), command.id);
    }
    return keymap;
  }

  resolveChord(chord: Chord): string | undefined {
    return this.byChord.get(chordId(chord));
  }

  resolveEvent(event: KeyboardEvent, isMac: boolean): string | undefined {
    const chord = chordFromEvent(event, isMac);
    return chord === null ? undefined : this.resolveChord(chord);
  }
}

/** Load overrides from storage. Never throws; malformed payloads yield {}. */
export function loadKeymapOverrides(storage: Pick<Storage, "getItem">): KeymapOverrides {
  let raw: string | null;
  try {
    raw = storage.getItem(KEYMAP_STORAGE_KEY);
  } catch {
    return {};
  }
  if (raw === null || raw === "") return {};

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return {};
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};

  const overrides: KeymapOverrides = {};
  for (const [id, value] of Object.entries(parsed)) {
    if (typeof value === "string" || value === null) overrides[id] = value;
  }
  return overrides;
}
