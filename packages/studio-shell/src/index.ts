/**
 * @brink/studio-shell — IDE shell infrastructure for brink-studio.
 *
 * Phases 1.1–1.3 of docs/studio-shell-spec.md: command registry, keymap
 * layer, global key handler, overlay primitive (§7.7), command palette,
 * tool-window registry (§7.1), shell layout store, region components
 * (docks/strips, §3/§5), and generated view-toggle commands (§5.2).
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
  useShellLayout,
  useToolWindows,
  type ShellContextValue,
  type ShellProviderProps,
} from "./shell-context.js";
export { Overlay, type OverlayProps } from "./overlay.js";
export { CommandPalette, PALETTE_COMMAND_ID, filterCommands } from "./palette.js";
export {
  DOCK_SECTION_IDS,
  ToolWindowRegistry,
  dockSectionId,
  type Dock,
  type DockSectionId,
  type Placement,
  type Section,
  type ToolWindowDescriptor,
} from "./toolwindow.js";
export {
  createShellLayoutStore,
  isToolWindowOpen,
  type LayoutTier,
  type ShellLayoutState,
  type ShellLayoutStore,
} from "./layout-store.js";
export { registerViewToggleCommands, viewToggleCommandId } from "./view-commands.js";
export { ShellFrame, type ShellFrameProps } from "./regions.js";
export { useTier } from "./use-tier.js";
