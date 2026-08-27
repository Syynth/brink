/**
 * @brink/studio-shell — shell context.
 *
 * ShellProvider owns the keymap lifecycle: it rebuilds the resolution table
 * whenever the registry changes (components register commands at mount) and
 * keeps the global key handler attached to the current keymap. It also owns
 * the shell layout store (spec §7.1), keeps it reconciled with the
 * tool-window registry, and generates the `view.toggle.<id>` commands from
 * it (spec §5.2). Shell components (palette, strips, …) reach all of this
 * through useShell() / useShellLayout() instead of prop-drilling.
 */

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { useStore } from "zustand";
import type { CommandRegistry } from "./command.js";
import { NotificationCenter, type NotificationState } from "./notifications.js";
import { Keymap, KeymapOverridesService, type KeymapOverrides } from "./keymap.js";
import { attachKeyHandler } from "./keyhandler.js";
import { ToolWindowRegistry, type ToolWindowDescriptor } from "./toolwindow.js";
import { StatusBarRegistry, type StatusBarItemDescriptor } from "./statusbar.js";
import {
  DocumentTypeRegistry,
  type DocumentRef,
  type DocumentTypeDescriptor,
} from "./document.js";
import {
  createEditorGroupsStore,
  type EditorGroupsState,
  type EditorGroupsStore,
} from "./editor-groups.js";
import { registerEditorGroupCommands } from "./editor-commands.js";
import { registerMaximizeCommands } from "./maximize-commands.js";
import {
  createShellLayoutStore,
  type ShellLayoutState,
  type ShellLayoutStore,
} from "./layout-store.js";
import { attachLayoutPersistence, loadLayoutSnapshot } from "./layout-persistence.js";
import { registerViewToggleCommands } from "./view-commands.js";
import { ThemeService, registerThemeCommands } from "./theme.js";

export interface ShellContextValue {
  commands: CommandRegistry;
  keymap: Keymap;
  isMac: boolean;
  toolWindows: ToolWindowRegistry;
  statusBarItems: StatusBarRegistry;
  documents: DocumentTypeRegistry;
  editorGroups: EditorGroupsStore;
  layout: ShellLayoutStore;
  notifications: NotificationCenter;
  themes: ThemeService;
  keymapOverrides: KeymapOverridesService;
  /**
   * The document Single File view shows beside the file (§7.2 keeps the
   * shell from knowing what a player is — the host names one). Undefined
   * means the view is just the file, full width.
   */
  companionDocument?: DocumentRef;
  /**
   * The element that fills the area in Continuous view. The host supplies it
   * because the ORDER files are read in is a project concept (binder order),
   * not something the shell can know — see `ContinuousView`.
   */
  continuousView?: ReactNode;
}

const ShellContext = createContext<ShellContextValue | null>(null);

export interface ShellProviderProps {
  commands: CommandRegistry;
  /** Tool-window registry; omit for shells without docks (tests). */
  toolWindows?: ToolWindowRegistry;
  /** Status-bar item registry (§7.3); omit for shells without one (tests). */
  statusBarItems?: StatusBarRegistry;
  /** Document-type registry (§7.8); omit for shells without documents (tests). */
  documents?: DocumentTypeRegistry;
  /**
   * Editor-groups store (§7.8); omit to let the provider own one. Pass an
   * instance when code outside the React tree opens documents (e.g. main.tsx
   * wiring the store's document opener).
   */
  editorGroups?: EditorGroupsStore;
  /**
   * Notification center (§7.5); omit to let the provider own one. Pass an
   * instance when producers outside the React tree need it (e.g. main.tsx
   * injecting the store→shell notifier bridge).
   */
  notifications?: NotificationCenter;
  /**
   * Theme service (§7.4); omit to let the provider own one over the
   * built-in themes. Pass an instance to register custom themes or to
   * inject test storage.
   */
  themes?: ThemeService;
  /**
   * Keymap-overrides service; omit to let the provider own one over
   * `storage`. Pass an instance when code outside the React tree edits
   * overrides (tests).
   */
  keymapOverrides?: KeymapOverridesService;
  /** Override storage for keymap overrides (tests); defaults to localStorage. */
  storage?: Pick<Storage, "getItem" | "setItem">;
  /** Override storage for layout persistence (tests); defaults to localStorage. */
  layoutStorage?: Pick<Storage, "getItem" | "setItem">;
  /**
   * The layout store. Pass one when code OUTSIDE the React tree needs it —
   * the studio registers the commands that take documents over the editor
   * area, and those run from `mountStudio`, not from a component. Omit it
   * and the provider owns one, as before.
   */
  layout?: ShellLayoutStore;
  /** Override platform detection (tests). */
  isMac?: boolean;
  /** The companion document for Single File view; see ShellContextValue. */
  companionDocument?: DocumentRef;
  /** The Continuous view's content; see ShellContextValue. */
  continuousView?: ReactNode;
  children: ReactNode;
}

export function ShellProvider({
  commands,
  toolWindows,
  statusBarItems,
  documents,
  editorGroups,
  notifications,
  themes,
  keymapOverrides,
  storage,
  layoutStorage,
  layout: layoutProp,
  isMac,
  companionDocument,
  continuousView,
  children,
}: ShellProviderProps) {
  const mac = isMac ?? detectMac();

  // Stable registry/store instances for the provider's lifetime.
  const [fallbackToolWindows] = useState(() => new ToolWindowRegistry());
  const registry = toolWindows ?? fallbackToolWindows;
  const [fallbackStatusBarItems] = useState(() => new StatusBarRegistry());
  const statusBar = statusBarItems ?? fallbackStatusBarItems;
  const [fallbackDocuments] = useState(() => new DocumentTypeRegistry());
  const documentTypes = documents ?? fallbackDocuments;
  const [fallbackEditorGroups] = useState<EditorGroupsStore>(() =>
    createEditorGroupsStore(),
  );
  const groups = editorGroups ?? fallbackEditorGroups;
  const [fallbackNotifications] = useState(() => new NotificationCenter());
  const notificationCenter = notifications ?? fallbackNotifications;
  // Theme service (§7.4): constructing reads the persisted selection, so
  // the root's data-theme is right on the first paint (like the layout
  // snapshot restore below).
  const [fallbackThemes] = useState(() => new ThemeService());
  const themeService = themes ?? fallbackThemes;
  // Layout: restore the persisted snapshot before the first render; the
  // registry-sync effect below then drops unknown ids / seeds new ones
  // (spec §7.1). Persistence is debounced writes of the durable subset.
  const [fallbackLayout] = useState<ShellLayoutStore>(() => createShellLayoutStore());
  const layout = layoutProp ?? fallbackLayout;
  // Restore into whichever store is in use — a HOST-SUPPLIED one included,
  // or injecting a store would silently cost the user their dock layout.
  // A state initializer rather than an effect because it has to land before
  // the first paint, not after it.
  useState(() => {
    const snapshot = loadLayoutSnapshot(layoutStorage ?? window.localStorage);
    if (snapshot !== null) layout.setState(snapshot);
    return null;
  });

  useEffect(
    () => attachLayoutPersistence(layout, layoutStorage ?? window.localStorage),
    [layout, layoutStorage],
  );

  // Keymap overrides (§6): the service loads the persisted JSON once at
  // construction; the Settings document (#93) edits through it, and the
  // subscription below feeds edits into the keymap rebuild — no reload.
  const [fallbackOverridesService] = useState(
    () => new KeymapOverridesService(storage),
  );
  const overridesService = keymapOverrides ?? fallbackOverridesService;
  const [overrides, setOverrides] = useState<KeymapOverrides>(
    () => overridesService.current,
  );

  useEffect(() => {
    setOverrides(overridesService.current);
    return overridesService.onDidChange(() =>
      setOverrides(overridesService.current),
    );
  }, [overridesService]);

  const [keymap, setKeymap] = useState<Keymap>(() =>
    Keymap.fromCommands(commands.list(), overrides),
  );

  useEffect(() => {
    const rebuild = () => setKeymap(Keymap.fromCommands(commands.list(), overrides));
    const unsubscribe = commands.onDidChange(rebuild);
    // Children's mount effects run before this parent effect, so commands
    // registered at mount (e.g. palette.toggle) predate the subscription —
    // rebuild once to pick them up.
    rebuild();
    return unsubscribe;
  }, [commands, overrides]);

  // Keep the layout store reconciled with the tool-window registry (seeding
  // placements/defaultOpen, dropping removed ids). Runs before the command
  // generation effect below so toggles always see placements.
  useEffect(() => {
    const sync = () => layout.getState().syncFromRegistry(registry.list());
    sync();
    return registry.onDidChange(sync);
  }, [registry, layout]);

  // The editor root area's occupant (decision log 2026-08-26). Registered
  // here rather than by the host because the layout store that holds the
  // choice is the provider's, and because every host gets the same views —
  // there is nothing project-specific to configure.
  useEffect(() => {
    const dispose = [
      commands.register({
        id: "view.editor.code",
        title: "View mode: Code (tabs and splits)",
        run: () => layout.getState().setEditorView("code"),
      }),
      commands.register({
        id: "view.editor.single",
        title: "View mode: Single File (one file beside the player)",
        run: () => layout.getState().setEditorView("single"),
      }),
      commands.register({
        id: "view.editor.continuous",
        title: "View mode: Continuous (every file as one manuscript)",
        run: () => layout.getState().setEditorView("continuous"),
      }),
    ];
    return () => {
      for (const d of dispose) d();
    };
  }, [commands, layout]);

  // Generate view.toggle.<id> commands (Mod-1…9 by registration order),
  // regenerating wholesale on registry changes.
  useEffect(() => {
    let dispose = registerViewToggleCommands(commands, registry.list(), layout);
    const unsubscribe = registry.onDidChange(() => {
      dispose();
      dispose = registerViewToggleCommands(commands, registry.list(), layout);
    });
    return () => {
      unsubscribe();
      dispose();
    };
  }, [commands, registry, layout]);

  useEffect(
    () => attachKeyHandler(window, commands, keymap, { isMac: mac }),
    [commands, keymap, mac],
  );

  // Maximize commands (spec §5.4): tool-window maximize (view.maximize) and
  // editor-group maximize (editor.maximizeGroup), registered together because
  // they are mutually exclusive. Escape restores either (ShellFrame).
  useEffect(
    () => registerMaximizeCommands(commands, layout, groups),
    [commands, layout, groups],
  );

  // Editor-group commands (spec §7.8): split, move tab, focus next group.
  useEffect(
    () => registerEditorGroupCommands(commands, groups),
    [commands, groups],
  );

  // Theme commands (§7.4): theme.select.<id>, palette-discoverable.
  useEffect(
    () => registerThemeCommands(commands, themeService),
    [commands, themeService],
  );

  const value = useMemo<ShellContextValue>(
    () => ({
      commands,
      keymap,
      isMac: mac,
      toolWindows: registry,
      statusBarItems: statusBar,
      documents: documentTypes,
      editorGroups: groups,
      layout,
      notifications: notificationCenter,
      themes: themeService,
      keymapOverrides: overridesService,
      companionDocument,
      continuousView,
    }),
    [
      commands,
      keymap,
      mac,
      registry,
      statusBar,
      documentTypes,
      groups,
      layout,
      notificationCenter,
      themeService,
      overridesService,
      companionDocument,],
  );

  return <ShellContext.Provider value={value}>{children}</ShellContext.Provider>;
}

export function useShell(): ShellContextValue {
  const value = useContext(ShellContext);
  if (value === null) {
    throw new Error("useShell() requires a <ShellProvider> ancestor");
  }
  return value;
}

/** Select from the shell layout store (re-renders on selected changes). */
export function useShellLayout<T>(selector: (state: ShellLayoutState) => T): T {
  const { layout } = useShell();
  return useStore(layout, selector);
}

/** The registered tool windows, in registration order (reactive). */
export function useToolWindows(): ToolWindowDescriptor[] {
  const { toolWindows } = useShell();
  const [list, setList] = useState<ToolWindowDescriptor[]>(() => toolWindows.list());
  useEffect(() => {
    setList(toolWindows.list());
    return toolWindows.onDidChange(() => setList(toolWindows.list()));
  }, [toolWindows]);
  return list;
}

/** Select from the editor-groups store (re-renders on selected changes). */
export function useEditorGroups<T>(selector: (state: EditorGroupsState) => T): T {
  const { editorGroups } = useShell();
  return useStore(editorGroups, selector);
}

/** The registered document types, in registration order (reactive). */
export function useDocumentTypes(): DocumentTypeDescriptor[] {
  const { documents } = useShell();
  const [list, setList] = useState<DocumentTypeDescriptor[]>(() => documents.list());
  useEffect(() => {
    setList(documents.list());
    return documents.onDidChange(() => setList(documents.list()));
  }, [documents]);
  return list;
}

/** The shell's notification center (spec §7.5). */
export function useNotifications(): NotificationCenter {
  return useShell().notifications;
}

/** The current theme id (reactive) — the root's data-theme value (§7.4). */
export function useThemeId(): string {
  const { themes } = useShell();
  return useSyncExternalStore(
    (onChange) => themes.onDidChange(onChange),
    () => themes.current,
  );
}

/** Reactive notification snapshot: visible toasts, overflow, history, unread. */
export function useNotificationState(): NotificationState {
  const { notifications } = useShell();
  return useSyncExternalStore(
    (onChange) => notifications.onDidChange(onChange),
    () => notifications.getState(),
  );
}

/** The registered status-bar items, in registration order (reactive). */
export function useStatusBarItems(): StatusBarItemDescriptor[] {
  const { statusBarItems } = useShell();
  const [list, setList] = useState<StatusBarItemDescriptor[]>(() =>
    statusBarItems.list(),
  );
  useEffect(() => {
    setList(statusBarItems.list());
    return statusBarItems.onDidChange(() => setList(statusBarItems.list()));
  }, [statusBarItems]);
  return list;
}

function detectMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iP(hone|ad|od)/.test(navigator.platform || navigator.userAgent);
}
