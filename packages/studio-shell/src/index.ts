/**
 * @brink/studio-shell — IDE shell infrastructure for brink-studio.
 *
 * Phase 1.1 of docs/studio-shell-spec.md: command registry, keymap layer,
 * global key handler. Regions, tool-window registry, palette, and the rest of
 * the shell land in later Phase 1 issues.
 */

export { CommandRegistry, HOST_ID_PREFIX, type Command } from "./command.js";
export {
  Keymap,
  KEYMAP_STORAGE_KEY,
  chordFromEvent,
  chordId,
  loadKeymapOverrides,
  parseKeybinding,
  type Chord,
  type KeymapOverrides,
} from "./keymap.js";
export { attachKeyHandler, type KeyHandlerOptions } from "./keyhandler.js";
