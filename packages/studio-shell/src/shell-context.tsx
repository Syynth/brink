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
  type ReactNode,
} from "react";
import { useStore } from "zustand";
import type { CommandRegistry } from "./command.js";
import { Keymap, loadKeymapOverrides, type KeymapOverrides } from "./keymap.js";
import { attachKeyHandler } from "./keyhandler.js";
import { ToolWindowRegistry, type ToolWindowDescriptor } from "./toolwindow.js";
import {
  createShellLayoutStore,
  type ShellLayoutState,
  type ShellLayoutStore,
} from "./layout-store.js";
import { registerViewToggleCommands } from "./view-commands.js";

export interface ShellContextValue {
  commands: CommandRegistry;
  keymap: Keymap;
  isMac: boolean;
  toolWindows: ToolWindowRegistry;
  layout: ShellLayoutStore;
}

const ShellContext = createContext<ShellContextValue | null>(null);

export interface ShellProviderProps {
  commands: CommandRegistry;
  /** Tool-window registry; omit for shells without docks (tests). */
  toolWindows?: ToolWindowRegistry;
  /** Override storage for keymap overrides (tests); defaults to localStorage. */
  storage?: Pick<Storage, "getItem">;
  /** Override platform detection (tests). */
  isMac?: boolean;
  children: ReactNode;
}

export function ShellProvider({
  commands,
  toolWindows,
  storage,
  isMac,
  children,
}: ShellProviderProps) {
  const mac = isMac ?? detectMac();

  // Stable registry/store instances for the provider's lifetime.
  const [fallbackToolWindows] = useState(() => new ToolWindowRegistry());
  const registry = toolWindows ?? fallbackToolWindows;
  const [layout] = useState<ShellLayoutStore>(() => createShellLayoutStore());

  // Overrides load once per provider; the Settings document (Phase 5) will
  // own editing them and can force a remount/reload.
  const [overrides] = useState<KeymapOverrides>(() =>
    loadKeymapOverrides(storage ?? window.localStorage),
  );

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

  const value = useMemo<ShellContextValue>(
    () => ({ commands, keymap, isMac: mac, toolWindows: registry, layout }),
    [commands, keymap, mac, registry, layout],
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

function detectMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iP(hone|ad|od)/.test(navigator.platform || navigator.userAgent);
}
