/**
 * @brink/studio-shell — IDE shell infrastructure for brink-studio.
 *
 * Phase 1 of docs/studio-shell-spec.md: command registry, keymap layer,
 * global key handler, overlay primitive (§7.7), command palette, tool-window
 * registry (§7.1), shell layout store, region components (docks/strips,
 * §3/§5), generated view-toggle commands (§5.2), the Location/navigation
 * protocol (§6.1), and the notification service (§7.5).
 */

import "./styles/index.css";

export { CommandRegistry, HOST_ID_PREFIX, assertHostId, type Command } from "./command.js";
export {
  installStudioExtensions,
  type StudioExtensionRegistries,
  type StudioExtensions,
} from "./extensions.js";
export {
  Keymap,
  KEYMAP_STORAGE_KEY,
  KeymapOverridesService,
  chordFromEvent,
  chordId,
  formatChord,
  loadKeymapOverrides,
  parseKeybinding,
  parseKeymapOverridesText,
  type Chord,
  type KeymapOverrides,
  type KeymapOverridesParseResult,
} from "./keymap.js";
export { attachKeyHandler, type KeyHandlerOptions } from "./keyhandler.js";
export {
  ShellProvider,
  useDocumentTypes,
  useEditorGroups,
  useNotificationState,
  useNotifications,
  useShell,
  useShellLayout,
  useStatusBarItems,
  useThemeId,
  useToolWindows,
  type ShellContextValue,
  type ShellProviderProps,
} from "./shell-context.js";
export {
  BUILTIN_THEMES,
  THEME_STORAGE_KEY,
  ThemeService,
  registerThemeCommands,
  themeSelectCommandId,
  type ThemeDescriptor,
} from "./theme.js";
export {
  DocumentTypeRegistry,
  documentKey,
  type DocumentRef,
  type DocumentTypeDescriptor,
  type DocumentViewProps,
} from "./document.js";
export {
  createEditorGroupsStore,
  findTab,
  focusedGroup,
  focusedTab,
  type EditorGroup,
  type EditorGroupsState,
  type EditorGroupsStore,
  type EditorTab,
  type OpenDocumentOptions,
} from "./editor-groups.js";
export { registerEditorGroupCommands } from "./editor-commands.js";
export {
  EDITOR_MAXIMIZE_GROUP_COMMAND_ID,
  VIEW_MAXIMIZE_COMMAND_ID,
  registerMaximizeCommands,
} from "./maximize-commands.js";
export { EditorArea } from "./editor-area.js";
export {
  MAX_VISIBLE_NOTIFICATIONS,
  NOTIFICATION_HISTORY_LIMIT,
  NotificationCenter,
  SEVERITY_TIMEOUTS,
  type Notification,
  type NotificationAction,
  type NotificationCenterOptions,
  type NotificationHandle,
  type NotificationInput,
  type NotificationSeverity,
  type NotificationState,
} from "./notifications.js";
export { NotificationBell, NotificationStack } from "./notification-ui.js";
export {
  StatusBarRegistry,
  statusBarGroups,
  type StatusBarAlignment,
  type StatusBarItemDescriptor,
} from "./statusbar.js";
export { Overlay, type OverlayProps } from "./overlay.js";
export { CommandPalette, PALETTE_COMMAND_ID, filterCommands } from "./palette.js";
export {
  QuickPick,
  rankQuickPickItems,
  type QuickPickItem,
  type QuickPickProps,
} from "./quickpick.js";
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
  ensureToolWindowOpen,
  isToolWindowOpen,
  type LayoutTier,
  type ShellLayoutState,
  type ShellLayoutStore,
} from "./layout-store.js";
export { registerViewToggleCommands, viewToggleCommandId } from "./view-commands.js";
export {
  LAYOUT_STORAGE_KEY,
  attachLayoutPersistence,
  loadLayoutSnapshot,
  snapshotLayout,
  type LayoutSnapshot,
} from "./layout-persistence.js";
export {
  DRAG_THRESHOLD_PX,
  StripDragGesture,
  exceedsDragThreshold,
  hitTestZone,
  placementFromZone,
  useStripDrag,
  type StripDragController,
  type StripDragHandlers,
  type StripDragPhase,
  type StripDragState,
  type ZoneRect,
} from "./strip-drag.js";
export { ShellFrame, ShellStatusBar } from "./regions.js";
export { HamburgerMenu, groupCommandsForMenu, type MenuGroup } from "./menu.js";
export { useTier } from "./use-tier.js";
export {
  EDITOR_REVEAL_COMMAND_ID,
  LocationResolvers,
  VIEW_REVEAL_COMMAND_ID,
  ViewRevealHandlers,
  resolveQualifiedSymbol,
  type Location,
  type LocationResolver,
  type OutlineFileLike,
  type OutlineSymbolLike,
  type SourceLocation,
  type Span,
} from "./location.js";
