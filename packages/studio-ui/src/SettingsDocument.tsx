/**
 * SettingsDocument — the "settings" document type (issue #93, spec §4
 * "Settings", §7.8).
 *
 * Settings as an editor document, not a modal (VS Code precedent). Static
 * UI over shell services — NOT session-bound, NOT compile-bound: closing
 * and reopening always works, and reopening focuses the existing tab (the
 * groups store's singleton reveal policy). Three sections:
 *
 * - Theme — radio picker over ThemeService.list(); applies live through
 *   select() and reflects external changes (palette command while the
 *   document is open) via onDidChange.
 * - Keymap overrides — a plain textarea over the user-override JSON the
 *   keymap layer persists under brink-studio.keymap.v1. Apply validates
 *   strictly (parseKeymapOverridesText) and writes through the shell's
 *   KeymapOverridesService, which rebuilds the live keymap; invalid JSON
 *   shows an inline error and saves nothing.
 * - Diagnostics — the one real severity flag: external-function checking
 *   ("error" / "off", the wasm session's set_external_check). Dispatched
 *   through the store action (never a raw wasm handle here) and persisted
 *   under brink-studio.diagnostics.v1; main.tsx restores it at bootstrap.
 */

import { useId, useState } from "react";
import {
  parseKeymapOverridesText,
  useShell,
  useShellLayout,
  useThemeId,
  type CommandRegistry,
  type DocumentRef,
  type DocumentViewProps,
  type EditorGroupsStore,
  type EditorViewId,
} from "@brink/studio-shell";
import type { ExternalCheckLevel } from "@brink/studio-store";
import {
  DEFAULT_APP_FONT_SIZE,
  MAX_APP_FONT_SIZE,
  MIN_APP_FONT_SIZE,
  clampAppFontSize,
  DEFAULT_EDITOR_FONT_SIZE,
  MAX_EDITOR_FONT_SIZE,
  MIN_EDITOR_FONT_SIZE,
  clampEditorFontSize,
} from "@brink-lang/editor";
import { useStudioStore } from "./StoreContext.js";

export const SETTINGS_TYPE_ID = "settings";
export const SETTINGS_DOC_ID = "settings";
export const OPEN_SETTINGS_COMMAND_ID = "settings.open";

/** The singleton DocumentRef — one stable identity, one tab. */
export function settingsRef(): DocumentRef {
  return { typeId: SETTINGS_TYPE_ID, docId: SETTINGS_DOC_ID, title: "Settings" };
}

/**
 * Register `settings.open` (palette: "Settings: Open", Mod-, per the VS
 * Code precedent). Opens pinned into the focused group; the groups store's
 * reveal policy focuses an existing tab wherever it lives.
 */
export function registerSettingsCommand(
  commands: CommandRegistry,
  editorGroups: EditorGroupsStore,
): () => void {
  return commands.register({
    id: OPEN_SETTINGS_COMMAND_ID,
    title: "Settings: Open",
    keybinding: "Mod-,",
    run: () => editorGroups.getState().openDocument(settingsRef(), { pinned: true }),
  });
}

// ── Diagnostics persistence ─────────────────────────────────────────
//
// Theme and keymap already persist through their own services/keys; the
// diagnostics flag gets its own versioned key. Load is lenient (corrupt
// payloads yield the defaults), like the other loaders.

export const DIAGNOSTICS_STORAGE_KEY = "brink-studio.diagnostics.v1";

export interface DiagnosticsSettings {
  externalCheck: ExternalCheckLevel;
}

const DEFAULT_DIAGNOSTICS: DiagnosticsSettings = { externalCheck: "error" };

/** Load persisted diagnostics settings. Never throws; defaults on garbage. */
export function loadDiagnosticsSettings(
  storage: Pick<Storage, "getItem">,
): DiagnosticsSettings {
  let raw: string | null;
  try {
    raw = storage.getItem(DIAGNOSTICS_STORAGE_KEY);
  } catch {
    return DEFAULT_DIAGNOSTICS;
  }
  if (raw === null || raw === "") return DEFAULT_DIAGNOSTICS;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_DIAGNOSTICS;
  }
  const externalCheck = (parsed as { externalCheck?: unknown } | null)?.externalCheck;
  return externalCheck === "off" ? { externalCheck: "off" } : DEFAULT_DIAGNOSTICS;
}

/** Persist diagnostics settings. Storage failures degrade to in-session. */
export function saveDiagnosticsSettings(
  storage: Pick<Storage, "setItem">,
  settings: DiagnosticsSettings,
): void {
  try {
    storage.setItem(DIAGNOSTICS_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Quota/denied storage — the setting still applies for this session.
  }
}

// ── Editor persistence ──────────────────────────────────────────────

export const EDITOR_STORAGE_KEY = "brink-studio.editor.v1";

export type FormGlyphMode = "off" | "hover" | "inline";

export interface EditorSettings {
  formGlyph: FormGlyphMode;
  autoOpenForm: boolean;
  /** Editor gutters (line numbers, rails, fold/play). Default ON; also the
   *  interim WebKit latency escape hatch on large projects (#3119). */
  showGutters: boolean;
  /** Editor text size in px (beta feedback 2026-08-25). Separate from any
   *  app-wide sizing: this is the prose you stare at, and authors want it
   *  bigger without inflating the chrome around it. */
  fontSize: number;
  /** App-wide UI text size in px — the base the whole type scale derives
   *  from. The other half of the two-knob ruling. */
  appFontSize: number;
}

const DEFAULT_EDITOR: EditorSettings = {
  formGlyph: "off",
  autoOpenForm: false,
  showGutters: true,
  fontSize: DEFAULT_EDITOR_FONT_SIZE,
  appFontSize: DEFAULT_APP_FONT_SIZE,
};

/** Load persisted editor settings. Never throws; defaults on garbage. */
export function loadEditorSettings(storage: Pick<Storage, "getItem">): EditorSettings {
  let raw: string | null;
  try {
    raw = storage.getItem(EDITOR_STORAGE_KEY);
  } catch {
    return DEFAULT_EDITOR;
  }
  if (raw === null || raw === "") return DEFAULT_EDITOR;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_EDITOR;
  }
  const obj = parsed as {
    formGlyph?: unknown;
    autoOpenForm?: unknown;
    showGutters?: unknown;
    fontSize?: unknown;
    appFontSize?: unknown;
  } | null;
  const glyph = obj?.formGlyph === "hover" || obj?.formGlyph === "inline" ? obj.formGlyph : "off";
  return {
    formGlyph: glyph,
    autoOpenForm: obj?.autoOpenForm === true,
    // Default ON: only an explicit false (a persisted opt-out) hides them.
    showGutters: obj?.showGutters !== false,
    // Garbage, out-of-range, and absent all land on the default.
    fontSize: clampEditorFontSize(obj?.fontSize),
    appFontSize: clampAppFontSize(obj?.appFontSize),
  };
}

/** Persist editor settings. Storage failures degrade to in-session. */
export function saveEditorSettings(
  storage: Pick<Storage, "setItem">,
  settings: EditorSettings,
): void {
  try {
    storage.setItem(EDITOR_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Quota/denied storage — the setting still applies for this session.
  }
}

// ── Sections ────────────────────────────────────────────────────────

/**
 * Which view fills the editor root area (decision log 2026-08-26). Its own
 * section rather than a field inside "Editor", because it does not configure
 * the editor — it chooses what is in the area the editor lives in.
 *
 * Radio buttons rather than a select: there are two or three of these ever,
 * each needs a sentence to explain what it is FOR, and a collapsed select
 * would hide exactly the part that helps you choose.
 */
function EditorViewSection() {
  const { layout } = useShell();
  const current = useShellLayout((s) => s.editorView);
  const views: { id: EditorViewId; label: string; hint: string }[] = [
    {
      id: "code",
      label: "Code",
      hint: "Tabs, groups and splits. For working across several files at once.",
    },
    {
      id: "single",
      label: "Single File",
      hint: "One file with the player beside it, and no tabs. For drafting a scene.",
    },
    {
      id: "continuous",
      label: "Continuous",
      hint: "Every file stacked in binder order, as one manuscript. For reading straight through.",
    },
  ];
  return (
    <section className="settings-section">
      <h2 className="settings-section-title">Editor view</h2>
      <p className="settings-section-hint">
        What fills the editor area. Switching keeps the file you are on.
      </p>
      <div className="settings-radio-group" role="radiogroup" aria-label="Editor view">
        {views.map((view) => (
          <label key={view.id} className="settings-radio settings-radio-explained">
            <input
              type="radio"
              name="brink-editor-view"
              value={view.id}
              checked={current === view.id}
              onChange={() => layout.getState().setEditorView(view.id)}
            />
            <span className="settings-radio-text">
              <span>{view.label}</span>
              <span className="settings-radio-hint">{view.hint}</span>
            </span>
          </label>
        ))}
      </div>
    </section>
  );
}

function ThemeSection() {
  const { themes } = useShell();
  const current = useThemeId();
  // Radio group name must be unique per mounted view (the singleton can
  // still be split-duplicated): same-name radios across views would
  // uncheck each other at the DOM level.
  const groupName = useId();

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">Theme</h2>
      <p className="settings-section-hint">
        Color theme for the whole studio. Applies immediately.
      </p>
      <div className="settings-radio-group" role="radiogroup" aria-label="Theme">
        {themes.list().map((theme) => (
          <label key={theme.id} className="settings-radio">
            <input
              type="radio"
              name={groupName}
              value={theme.id}
              checked={current === theme.id}
              onChange={() => void themes.select(theme.id)}
            />
            <span>{theme.label}</span>
          </label>
        ))}
      </div>
    </section>
  );
}

function KeymapSection() {
  const { keymapOverrides } = useShell();
  const [text, setText] = useState(() =>
    JSON.stringify(keymapOverrides.current, null, 2),
  );
  const [error, setError] = useState<string | null>(null);

  const apply = (): void => {
    const result = parseKeymapOverridesText(text);
    if (!result.ok) {
      setError(result.error);
      return;
    }
    setError(null);
    keymapOverrides.set(result.overrides);
  };

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">Keymap overrides</h2>
      <p className="settings-section-hint">
        JSON mapping a command id to a keybinding ({'"Mod-K"'}), an array of
        keybindings, or <code>null</code> to unbind. Overrides replace the
        command{"'"}s default bindings and take effect on Apply.
      </p>
      <textarea
        className="settings-json"
        value={text}
        onChange={(event) => setText(event.target.value)}
        spellCheck={false}
        rows={8}
        aria-label="Keymap overrides JSON"
      />
      {error !== null && (
        <p className="settings-error" role="alert">
          {error}
        </p>
      )}
      <button type="button" className="settings-apply" onClick={apply}>
        Apply
      </button>
    </section>
  );
}

function DiagnosticsSection() {
  const externalCheck = useStudioStore((s) => s.externalCheck);
  const setExternalCheck = useStudioStore((s) => s.setExternalCheck);
  const selectId = useId();

  const onChange = (level: ExternalCheckLevel): void => {
    setExternalCheck(level);
    saveDiagnosticsSettings(window.localStorage, { externalCheck: level });
  };

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">Diagnostics</h2>
      <p className="settings-section-hint">
        Severity of external-function checks against a registered host
        manifest. Recompiles on change.
      </p>
      <div className="settings-field">
        <label htmlFor={selectId}>External function checking</label>
        <select
          id={selectId}
          className="settings-select"
          value={externalCheck}
          onChange={(event) => onChange(event.target.value as ExternalCheckLevel)}
        >
          <option value="error">Error</option>
          <option value="off">Off</option>
        </select>
      </div>
    </section>
  );
}

function EditorSection() {
  const formGlyph = useStudioStore((s) => s.formGlyph);
  const setFormGlyph = useStudioStore((s) => s.setFormGlyph);
  const autoOpenForm = useStudioStore((s) => s.autoOpenForm);
  const setAutoOpenForm = useStudioStore((s) => s.setAutoOpenForm);
  const showGutters = useStudioStore((s) => s.showGutters);
  const setShowGutters = useStudioStore((s) => s.setShowGutters);
  const fontSize = useStudioStore((s) => s.editorFontSize);
  const setEditorFontSize = useStudioStore((s) => s.setEditorFontSize);
  const appFontSize = useStudioStore((s) => s.appFontSize);
  const setAppFontSize = useStudioStore((s) => s.setAppFontSize);
  const selectId = useId();
  const autoId = useId();
  const guttersId = useId();
  const fontSizeId = useId();
  const appFontSizeId = useId();

  const onGlyphChange = (mode: FormGlyphMode): void => {
    setFormGlyph(mode);
    saveEditorSettings(window.localStorage, {
      formGlyph: mode,
      autoOpenForm,
      showGutters,
      fontSize,
      appFontSize,
    });
  };
  const onAutoChange = (on: boolean): void => {
    setAutoOpenForm(on);
    saveEditorSettings(window.localStorage, {
      formGlyph,
      autoOpenForm: on,
      showGutters,
      fontSize,
      appFontSize,
    });
  };
  const onGuttersChange = (on: boolean): void => {
    setShowGutters(on);
    saveEditorSettings(window.localStorage, {
      formGlyph,
      autoOpenForm,
      showGutters: on,
      fontSize,
      appFontSize,
    });
  };
  const onFontSizeChange = (px: number): void => {
    const next = clampEditorFontSize(px);
    setEditorFontSize(next);
    saveEditorSettings(window.localStorage, {
      formGlyph,
      autoOpenForm,
      showGutters,
      fontSize: next,
      appFontSize,
    });
  };
  const onAppFontSizeChange = (px: number): void => {
    const next = clampAppFontSize(px);
    setAppFontSize(next);
    saveEditorSettings(window.localStorage, {
      formGlyph,
      autoOpenForm,
      showGutters,
      fontSize,
      appFontSize: next,
    });
  };

  return (
    <section className="settings-section">
      <h2 className="settings-section-title">Editor</h2>
      <p className="settings-section-hint">
        The inline argument-form glyph (the clickable mark after a function name).
        The hover card{"'"}s {'"'}edit arguments{'"'} action and the Mod-Shift-A
        shortcut are always available regardless of this setting.
      </p>
      <div className="settings-field">
        <label htmlFor={selectId}>Argument-form glyph</label>
        <select
          id={selectId}
          className="settings-select"
          value={formGlyph}
          onChange={(event) => onGlyphChange(event.target.value as FormGlyphMode)}
        >
          <option value="off">Off (card + shortcut only)</option>
          <option value="hover">On line hover</option>
          <option value="inline">Always visible</option>
        </select>
      </div>
      <div className="settings-field">
        <label htmlFor={autoId}>
          <input
            id={autoId}
            type="checkbox"
            checked={autoOpenForm}
            onChange={(event) => onAutoChange(event.target.checked)}
            style={{ marginRight: 8 }}
          />
          Open the form when accepting a function completion
        </label>
      </div>
      <div className="settings-field">
        <label htmlFor={guttersId}>
          <input
            id={guttersId}
            type="checkbox"
            checked={showGutters}
            onChange={(event) => onGuttersChange(event.target.checked)}
            style={{ marginRight: 8 }}
          />
          Show editor gutters (line numbers, structure rails, fold/play markers)
        </label>
      </div>
      <div className="settings-field">
        <label htmlFor={fontSizeId}>
          Editor font size
          <input
            id={fontSizeId}
            type="number"
            min={MIN_EDITOR_FONT_SIZE}
            max={MAX_EDITOR_FONT_SIZE}
            value={fontSize}
            onChange={(event) => onFontSizeChange(Number(event.target.value))}
            style={{ marginLeft: 8, width: 64 }}
          />
        </label>
        <p className="settings-section-hint">
          {MIN_EDITOR_FONT_SIZE}–{MAX_EDITOR_FONT_SIZE} px (default{" "}
          {DEFAULT_EDITOR_FONT_SIZE}). Also Mod-= / Mod-- / Mod-0 while editing.
        </p>
      </div>
      <div className="settings-field">
        <label htmlFor={appFontSizeId}>
          App font size
          <input
            id={appFontSizeId}
            type="number"
            min={MIN_APP_FONT_SIZE}
            max={MAX_APP_FONT_SIZE}
            value={appFontSize}
            onChange={(event) => onAppFontSizeChange(Number(event.target.value))}
            style={{ marginLeft: 8, width: 64 }}
          />
        </label>
        <p className="settings-section-hint">
          {MIN_APP_FONT_SIZE}–{MAX_APP_FONT_SIZE} px (default{" "}
          {DEFAULT_APP_FONT_SIZE}). Scales panels, menus, and labels — the
          whole type scale moves with it; the editor keeps its own size.
        </p>
      </div>
    </section>
  );
}

export function SettingsDocument(_props: DocumentViewProps) {
  return (
    <div className="settings-doc">
      <div className="settings-doc-inner">
        <ThemeSection />
        <EditorViewSection />
        <EditorSection />
        <KeymapSection />
        <DiagnosticsSection />
      </div>
    </div>
  );
}
