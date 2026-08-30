/**
 * The editor's named actions as shell commands (Settings ▸ Keymap).
 *
 * Rename, find references, code actions, the argument form and the element
 * picker used to exist only as chords hardcoded inside CodeMirror
 * extensions — invisible to the keymap table (which lists
 * `commands.list()`) and unrebindable. They now register here like any
 * other command, and a rebind flows BACK into the editors: the shell's
 * overrides are the one source of truth, and the CM6 keymap is
 * reconfigured live to match, so the table can never show a chord the
 * editor disagrees with.
 *
 * Dispatch stays conflict-free by construction. Inside a focused editor
 * the CM6 keymap consumes the chord and `preventDefault`s, so the shell
 * key handler (which skips `defaultPrevented` events) never double-fires;
 * outside one — Binder focus, or the palette — the shell command runs the
 * action on the focused group's editor through
 * `DocumentSessions.runEditorAction`.
 */

import { EDITOR_ACTIONS, type EditorActionId, type EditorActionKeys } from "@brink-lang/editor";
import {
  effectiveChords,
  type Chord,
  type CommandRegistry,
} from "@brink/studio-shell";

/** The two seams this module drives — typed narrowly so tests stub them. */
export interface EditorActionHost {
  runEditorAction(id: EditorActionId): boolean;
  setEditorActionKeys(keys: EditorActionKeys): void;
}

/** The overrides surface this module reads — `KeymapOverridesService`'s. */
export interface OverridesSource {
  readonly current: Record<string, string | readonly string[] | null>;
  onDidChange(listener: () => void): () => void;
}

/**
 * A shell chord in CodeMirror keybinding spelling.
 *
 * The two dialects agree on almost everything (`Mod-`/`Alt-`/`Shift-`
 * prefixes, `F2`, `Enter`, `ArrowUp` — CM6 matches `event.key`, and the
 * shell's parser lowercases the same names on the way in). The deltas:
 * single letters go out lowercase (CM6's canonical form — it matches the
 * unshifted `event.key`), and the shell's `Minus`/`Space` aliases become
 * the literal key (`Mod--` is valid CM6: its splitter is `-` not at
 * end-of-string).
 */
export function chordToCm6Key(chord: Chord): string {
  const parts: string[] = [];
  if (chord.mod) parts.push("Mod");
  if (chord.alt) parts.push("Alt");
  if (chord.shift) parts.push("Shift");
  parts.push(cm6KeyName(chord.key));
  return parts.join("-");
}

function cm6KeyName(key: string): string {
  if (key === " ") return "Space";
  if (key.length === 1) return key;
  if (/^f\d{1,2}$/.test(key)) return key.toUpperCase();
  return CM6_NAMED_KEYS[key] ?? key;
}

/** `event.key` spellings for the multi-character keys the shell lowercased. */
const CM6_NAMED_KEYS: Record<string, string> = {
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

/**
 * Register the five as commands and keep the editors' chords in sync with
 * the shell overrides. Returns a disposer.
 */
export function registerEditorActionCommands(
  commands: CommandRegistry,
  host: EditorActionHost,
  overrides: OverridesSource,
): () => void {
  const disposers = (Object.keys(EDITOR_ACTIONS) as EditorActionId[]).map((id) =>
    commands.register({
      id,
      title: `Editor: ${EDITOR_ACTIONS[id].title}`,
      // The editor's shipped default, verbatim — the spellings are chosen
      // to be valid in both keybinding dialects (see editor-actions.ts),
      // so the shell declares exactly what the editor binds.
      keybinding: EDITOR_ACTIONS[id].key,
      run: () => {
        host.runEditorAction(id);
      },
    }),
  );

  // Push the overrides' current truth into the editors — now (an author
  // with saved rebinds from a previous session must get them at mount, not
  // at their first edit) and on every change.
  const sync = (): void => {
    const keys: EditorActionKeys = {};
    for (const id of Object.keys(EDITOR_ACTIONS) as EditorActionId[]) {
      const chords = effectiveChords(
        { id, title: EDITOR_ACTIONS[id].title, keybinding: EDITOR_ACTIONS[id].key },
        overrides.current,
      );
      keys[id] = chords.length === 0 ? null : chords.map(chordToCm6Key);
    }
    host.setEditorActionKeys(keys);
  };
  sync();
  const unsubscribe = overrides.onDidChange(sync);

  return () => {
    unsubscribe();
    for (const dispose of disposers) dispose();
  };
}
