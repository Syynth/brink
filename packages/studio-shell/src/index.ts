/**
 * @brink/studio-shell — IDE shell infrastructure for brink-studio.
 *
 * Phases 1.1–1.2 of docs/studio-shell-spec.md: command registry, keymap
 * layer, global key handler, overlay primitive (§7.7), command palette.
 * Regions/tool-window registry land with the dock shell (#80).
 */

export { CommandRegistry, HOST_ID_PREFIX, type Command } from "./command.js";
export {
  Keymap,
  KEYMAP_STORAGE_KEY,
  chordFromEvent,
  chordId,
  formatChord,
  loadKeymapOverrides,
  parseKeybinding,
  type Chord,
  type KeymapOverrides,
} from "./keymap.js";
export { attachKeyHandler, type KeyHandlerOptions } from "./keyhandler.js";
export {
  ShellProvider,
  useShell,
  type ShellContextValue,
  type ShellProviderProps,
} from "./shell-context.js";
export { Overlay, type OverlayProps } from "./overlay.js";
export { CommandPalette, PALETTE_COMMAND_ID, filterCommands } from "./palette.js";
