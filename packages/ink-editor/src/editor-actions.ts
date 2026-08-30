/**
 * The editor's named actions — rename, find references, code actions, the
 * argument form, the element picker — as ONE registry with rebindable keys.
 *
 * Before this module each feature extension baked its chord into its own
 * `keymap.of([...])`, which had two costs. The action was invisible to the
 * shell's keymap surface (Settings ▸ Keymap lists `commands.list()`, and
 * these never entered a registry), and it was unrebindable: nothing could
 * reach a chord captured inside an extension's closure.
 *
 * The split here is runner vs chord. Each feature extension still OWNS its
 * behaviour — its run body closes over its options exactly as before — but
 * registers it under an {@link EditorActionId} through the
 * {@link editorActionRunners} facet instead of binding a key itself. The
 * chords live in one keymap, in a compartment, so the host can rebind them
 * live ({@link setEditorActionKeys}) and invoke them imperatively
 * ({@link runEditorAction}) from a palette or a shell keybinding.
 *
 * The default spellings below are deliberately valid in BOTH keybinding
 * dialects — CodeMirror's (`keymap.of`) and the studio shell's
 * (`parseKeybinding`) — so the shell can declare them verbatim as command
 * defaults and the two dispatch paths can never disagree about what a
 * default is. (`Mod-.` replaced the old `Ctrl-.`/`mac: Cmd-.` pair; CM6's
 * `Mod-` means exactly that platform split.)
 */

import { Compartment, Facet, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";

/** The rebindable editor actions, with their shipped default chords. */
export const EDITOR_ACTIONS = {
  "editor.renameSymbol": { title: "Rename Symbol", key: "F2" },
  "editor.findReferences": { title: "Find References", key: "Shift-Alt-f" },
  "editor.codeActions": { title: "Code Actions", key: "Mod-." },
  "editor.argumentForm": { title: "Edit Arguments", key: "Mod-Shift-a" },
  "editor.insertElement": { title: "Insert Element", key: "Alt-Enter" },
} as const;

export type EditorActionId = keyof typeof EDITOR_ACTIONS;

/** id → chords, `null`/empty meaning explicitly unbound. */
export type EditorActionKeys = Partial<Record<EditorActionId, readonly string[] | null>>;

/**
 * Feature extensions provide their run bodies here.
 *
 * A run returns false when it has nothing to do at the cursor, letting the
 * chord fall through — the same contract `keymap.of` runs already had.
 */
export const editorActionRunners = Facet.define<{
  id: EditorActionId;
  run: (view: EditorView) => boolean;
}>();

/**
 * Run `id`'s action on `view` now — the palette/shell entry point.
 *
 * False when the view has no runner for it (the feature is not wired in
 * this editor) — never a throw, so a shell command can call this blind.
 */
export function runEditorAction(view: EditorView, id: EditorActionId): boolean {
  for (const runner of view.state.facet(editorActionRunners)) {
    if (runner.id === id) return runner.run(view);
  }
  return false;
}

/** One shared compartment: reconfigurable per state, like `dialectCompartment`. */
const actionKeysCompartment = new Compartment();

/** The keymap for `keys`, dispatching through the runners facet. */
function actionKeymap(keys: EditorActionKeys): Extension {
  const bindings = [];
  for (const id of Object.keys(EDITOR_ACTIONS) as EditorActionId[]) {
    const configured = Object.prototype.hasOwnProperty.call(keys, id)
      ? keys[id]
      : [EDITOR_ACTIONS[id].key];
    if (configured === null || configured === undefined) continue;
    for (const key of configured) {
      bindings.push({ key, run: (view: EditorView) => runEditorAction(view, id) });
    }
  }
  return keymap.of(bindings);
}

/**
 * The keymap half of the actions, at the shipped defaults — part of the
 * editor baseline, so an embedder that never touches keys gets exactly the
 * bindings the features shipped with.
 */
export function editorActionKeymap(): Extension {
  return actionKeysCompartment.of(actionKeymap({}));
}

/**
 * Rebind `view`'s action keys — the whole map at once, matching how the
 * shell's overrides replace a command's whole binding set.
 *
 * A no-op on a view whose baseline predates {@link editorActionKeymap}
 * (nothing to reconfigure), rather than an error: the caller is a
 * settings broadcast that must not die on one stale view.
 */
export function setEditorActionKeys(view: EditorView, keys: EditorActionKeys): void {
  view.dispatch({ effects: actionKeysCompartment.reconfigure(actionKeymap(keys)) });
}
