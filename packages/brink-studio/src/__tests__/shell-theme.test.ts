/**
 * Theme service tests (issue #92, spec §7.4): registry surface, selection,
 * persistence round-trip through the versioned localStorage key, and the
 * generated theme.select.<id> commands.
 */

import { describe, expect, it, vi } from "vitest";
import {
  BUILTIN_THEMES,
  CommandRegistry,
  THEME_STORAGE_KEY,
  ThemeService,
  registerThemeCommands,
  themeSelectCommandId,
} from "@brink/studio-shell";

function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    dump: () => map.get(THEME_STORAGE_KEY) ?? null,
  };
}

describe("ThemeService", () => {
  it("defaults to the first theme and lists the built-ins", () => {
    const themes = new ThemeService(BUILTIN_THEMES, memoryStorage());
    expect(themes.current).toBe("mocha");
    expect(themes.list().map((t) => t.id)).toEqual(["mocha", "latte"]);
    expect(themes.list().map((t) => t.label)).toEqual([
      "Catppuccin Mocha",
      "Catppuccin Latte",
    ]);
  });

  it("select() switches, notifies, and persists under the versioned key", () => {
    const storage = memoryStorage();
    const themes = new ThemeService(BUILTIN_THEMES, storage);
    const listener = vi.fn();
    themes.onDidChange(listener);

    expect(themes.select("latte")).toBe(true);
    expect(themes.current).toBe("latte");
    expect(listener).toHaveBeenCalledTimes(1);
    expect(storage.dump()).toBe("latte");
  });

  it("ignores unknown ids (no change, no notify)", () => {
    const storage = memoryStorage();
    const themes = new ThemeService(BUILTIN_THEMES, storage);
    const listener = vi.fn();
    themes.onDidChange(listener);

    expect(themes.select("dracula")).toBe(false);
    expect(themes.current).toBe("mocha");
    expect(listener).not.toHaveBeenCalled();
    expect(storage.dump()).toBeNull();
  });

  it("re-selecting the current theme is a no-op (no notify)", () => {
    const themes = new ThemeService(BUILTIN_THEMES, memoryStorage());
    const listener = vi.fn();
    themes.onDidChange(listener);
    expect(themes.select("mocha")).toBe(true);
    expect(listener).not.toHaveBeenCalled();
  });

  it("round-trips the persisted choice into a fresh service", () => {
    const storage = memoryStorage();
    new ThemeService(BUILTIN_THEMES, storage).select("latte");
    const restored = new ThemeService(BUILTIN_THEMES, storage);
    expect(restored.current).toBe("latte");
  });

  it("ignores a persisted id that is no longer a known theme", () => {
    const storage = memoryStorage({ [THEME_STORAGE_KEY]: "dracula" });
    expect(new ThemeService(BUILTIN_THEMES, storage).current).toBe("mocha");
  });

  it("degrades to in-session selection when storage throws", () => {
    const denied = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    const themes = new ThemeService(BUILTIN_THEMES, denied);
    expect(themes.current).toBe("mocha");
    expect(themes.select("latte")).toBe(true);
    expect(themes.current).toBe("latte");
  });

  it("unsubscribe stops notifications", () => {
    const themes = new ThemeService(BUILTIN_THEMES, memoryStorage());
    const listener = vi.fn();
    const unsubscribe = themes.onDidChange(listener);
    unsubscribe();
    themes.select("latte");
    expect(listener).not.toHaveBeenCalled();
  });
});

describe("theme commands (spec §7.4)", () => {
  it("registers one palette-discoverable theme.select.<id> per theme", () => {
    const commands = new CommandRegistry();
    const themes = new ThemeService(BUILTIN_THEMES, memoryStorage());
    registerThemeCommands(commands, themes);

    expect(commands.get(themeSelectCommandId("mocha"))?.title).toBe(
      "Theme: Catppuccin Mocha",
    );
    expect(commands.get(themeSelectCommandId("latte"))?.title).toBe(
      "Theme: Catppuccin Latte",
    );
  });

  it("dispatching switches the theme; the disposer unregisters", () => {
    const commands = new CommandRegistry();
    const storage = memoryStorage();
    const themes = new ThemeService(BUILTIN_THEMES, storage);
    const dispose = registerThemeCommands(commands, themes);

    expect(commands.dispatch("theme.select.latte")).toBe(true);
    expect(themes.current).toBe("latte");
    expect(storage.dump()).toBe("latte");
    expect(commands.dispatch("theme.select.mocha")).toBe(true);
    expect(themes.current).toBe("mocha");

    dispose();
    expect(commands.get("theme.select.mocha")).toBeUndefined();
    expect(commands.dispatch("theme.select.latte")).toBe(false);
  });
});
