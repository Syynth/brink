/**
 * Stable Settings section ids (#3174).
 *
 * Their own module, with no imports, because both the section registry and
 * the command that opens Settings need them — and having the command reach
 * into the registry would make a cycle
 * (`SettingsDocument` → `settingsSections` → `SettingsDocument`).
 *
 * `settingsSection` is `string | null` where **null means CLOSED**. Every
 * door therefore names a section rather than passing null and hoping: the
 * palette command opens at `general`, the Binder's `brink.toml` row at
 * `general`, and the Problems panel's "Configure Exxx…" at `diagnostics`.
 * The first version of this used null for "open at the default", which is
 * the same value as closed — so the command opened and immediately closed.
 */
export const SETTINGS_SECTION_IDS = {
  general: "general",
  diagnostics: "diagnostics",
  editor: "editor",
  appearance: "appearance",
  keymap: "keymap",
  external: "external",
} as const;

/** Where a door with no preference of its own lands. */
export const DEFAULT_SETTINGS_SECTION: string = SETTINGS_SECTION_IDS.general;
