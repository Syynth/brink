/**
 * @brink/studio-shell — shell context.
 *
 * ShellProvider owns the keymap lifecycle: it rebuilds the resolution table
 * whenever the registry changes (components register commands at mount) and
 * keeps the global key handler attached to the current keymap. Shell
 * components (palette, strips, …) reach the registry/keymap through
 * useShell() instead of prop-drilling.
 */

import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import type { CommandRegistry } from "./command.js";
import { Keymap, loadKeymapOverrides, type KeymapOverrides } from "./keymap.js";
import { attachKeyHandler } from "./keyhandler.js";

export interface ShellContextValue {
  commands: CommandRegistry;
  keymap: Keymap;
  isMac: boolean;
}

const ShellContext = createContext<ShellContextValue | null>(null);

export interface ShellProviderProps {
  commands: CommandRegistry;
  /** Override storage for keymap overrides (tests); defaults to localStorage. */
  storage?: Pick<Storage, "getItem">;
  /** Override platform detection (tests). */
  isMac?: boolean;
  children: ReactNode;
}

export function ShellProvider({ commands, storage, isMac, children }: ShellProviderProps) {
  const mac = isMac ?? detectMac();

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

  useEffect(
    () => attachKeyHandler(window, commands, keymap, { isMac: mac }),
    [commands, keymap, mac],
  );

  const value = useMemo<ShellContextValue>(
    () => ({ commands, keymap, isMac: mac }),
    [commands, keymap, mac],
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

function detectMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iP(hone|ad|od)/.test(navigator.platform || navigator.userAgent);
}
