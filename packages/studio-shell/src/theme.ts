/**
 * @brink/studio-shell — theme service (docs/studio-shell-spec.md §7.4).
 *
 * Themes are CSS files defining the semantic --bs-* token values under a
 * `[data-theme="<id>"]` scope on the .brink-studio root (Mocha doubles as
 * the bare-class default/fallback). This module owns the registry side:
 * the known themes, the current selection, persistence, and the
 * `theme.select.<id>` commands — switching is just flipping the data-theme
 * attribute, so it works at runtime without a reload.
 *
 * The Settings document (#93) consumes the same surface: list(), current,
 * select(), onDidChange().
 */

import type { CommandRegistry } from "./command.js";

export interface ThemeDescriptor {
  /** Stable id, also the data-theme attribute value, e.g. "mocha". */
  id: string;
  /** Display label, e.g. "Catppuccin Mocha". */
  label: string;
}

/** The built-in themes. The first entry is the default. */
export const BUILTIN_THEMES: readonly ThemeDescriptor[] = [
  { id: "mocha", label: "Catppuccin Mocha" },
  { id: "latte", label: "Catppuccin Latte" },
  // Theme ruling 2026-08-25 (docs/editor-color-design.md): the
  // writing-first colorway, plus faithful ports of Inky's two looks.
  { id: "manuscript", label: "Manuscript" },
  { id: "inky", label: "Inky" },
  { id: "inky-dark", label: "Inky Dark" },
];

export const THEME_STORAGE_KEY = "brink-studio.theme.v1";

type StorageLike = Pick<Storage, "getItem" | "setItem">;

/**
 * Owns the theme list and the current selection. Constructing reads the
 * persisted choice synchronously, so a provider created before first render
 * already carries it — the root applies data-theme on the initial paint,
 * like the layout snapshot (§7.1). Selection persists immediately (no
 * debounce — switches are rare and atomic). Storage failures degrade to
 * in-session selection.
 */
export class ThemeService {
  private readonly themes: readonly ThemeDescriptor[];
  private readonly storage: StorageLike | null;
  private currentId: string;
  private readonly changeListeners = new Set<() => void>();

  constructor(
    themes: readonly ThemeDescriptor[] = BUILTIN_THEMES,
    storage: StorageLike | null = defaultStorage(),
  ) {
    if (themes.length === 0) throw new Error("ThemeService needs at least one theme");
    this.themes = themes;
    this.storage = storage;
    this.currentId = this.loadPersisted() ?? themes[0].id;
  }

  /** The registered themes, in registration order. */
  list(): ThemeDescriptor[] {
    return [...this.themes];
  }

  /** The current theme id (always one of list()). */
  get current(): string {
    return this.currentId;
  }

  /**
   * Switch themes. Unknown ids are ignored (a stale persisted value or a
   * bad command arg must not blank the UI). Returns whether it applied.
   */
  select(id: string): boolean {
    if (!this.themes.some((t) => t.id === id)) return false;
    if (id !== this.currentId) {
      this.currentId = id;
      this.persist(id);
      for (const listener of this.changeListeners) listener();
    }
    return true;
  }

  /** Subscribe to selection changes. Returns an unsubscribe function. */
  onDidChange(listener: () => void): () => void {
    this.changeListeners.add(listener);
    return () => {
      this.changeListeners.delete(listener);
    };
  }

  private loadPersisted(): string | null {
    let raw: string | null;
    try {
      raw = this.storage?.getItem(THEME_STORAGE_KEY) ?? null;
    } catch {
      return null;
    }
    return raw !== null && this.themes.some((t) => t.id === raw) ? raw : null;
  }

  private persist(id: string): void {
    try {
      this.storage?.setItem(THEME_STORAGE_KEY, id);
    } catch {
      // Quota/denied storage — selection degrades to in-session.
    }
  }
}

/**
 * Register one `theme.select.<id>` command per theme (palette: "Theme:
 * Catppuccin Mocha", …). Returns a disposer that unregisters them all.
 */
export function registerThemeCommands(
  commands: CommandRegistry,
  themes: ThemeService,
): () => void {
  const disposers = themes.list().map((theme) =>
    commands.register({
      id: themeSelectCommandId(theme.id),
      title: `Theme: ${theme.label}`,
      run: () => void themes.select(theme.id),
    }),
  );
  return () => {
    for (const dispose of disposers) dispose();
  };
}

/** Command id for selecting a theme, e.g. "theme.select.mocha". */
export function themeSelectCommandId(themeId: string): string {
  return `theme.select.${themeId}`;
}

function defaultStorage(): StorageLike | null {
  return typeof window === "undefined" ? null : window.localStorage;
}
