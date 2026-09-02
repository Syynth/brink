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

import { useId, useMemo, useState } from "react";
import {
  parseKeymapOverridesText,
  useShell,
  useShellLayout,
  useThemeId,
  type CommandRegistry,
  type DocumentRef,
  type DocumentViewProps,
  type ShellLayoutStore,
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
  DialectParser,
  runsOf,
} from "@brink-lang/editor";
import { useStudioStore } from "./StoreContext.js";
import {
  SettingsGroup,
  SettingsRow,
  SettingsStepper,
  SettingsToggle,
} from "./SettingsRow.js";
import { DEFAULT_SETTINGS_SECTION } from "./settingsSectionIds.js";
import { isConfigPath } from "./ConfigFormPanel.js";
import { LintSettings } from "./LintSettings.js";
import { ThemePicker } from "./ThemePicker.js";
import { InkFileDocument, inkFileRef } from "./InkFileDocument.js";

/**
 * The project's `brink.toml`, rendered inside Settings (#3166, ruled
 * 2026-08-27: clicking it in the Binder opens the Settings takeover in
 * every view, because Continuous view renders the MANUSCRIPT and the
 * config file is deliberately not part of it).
 *
 * This mounts the real ink-file document, not a copy of its form. That is
 * the load-bearing part: `ConfigFormPanel` models four keys, and #3015
 * ruled the raw text beneath it to be the escape hatch for everything it
 * does not — which now includes `drafts` (#3145) and `indent` (#3149).
 * A form-only section here would have made those two uneditable from the
 * studio entirely, which is a regression dressed as a feature.
 *
 * Renders nothing when the project has no `brink.toml`; there is no
 * "create one" affordance here, since the Binder already owns file
 * creation.
 */
export function ProjectSection({ groupId }: { groupId: string }) {
  const outline = useStudioStore((s) => s.outline);
  const configPath = useMemo(
    () => outline.find((f) => !f.mounted && isConfigPath(f.path))?.path ?? null,
    [outline],
  );
  if (configPath === null) return null;
  return (
    <section className="settings-section settings-project">
      <p className="settings-section-hint">
        <code>{configPath}</code> — the form covers the common keys; the text below it is
        the escape hatch for everything else.
      </p>
      <div className="settings-project-doc">
        <InkFileDocument
          doc={inkFileRef({ kind: "file", path: configPath })}
          groupId={groupId}
          active
        />
      </div>
    </section>
  );
}

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
  openSettings: (section: string | null) => void,
): () => void {
  return commands.register({
    id: OPEN_SETTINGS_COMMAND_ID,
    title: "Settings: Open",
    keybinding: "Mod-,",
    // A MODAL, not an editor occupant (ruled 2026-08-27, #3174). It was a
    // takeover while Settings was small — right at the time, because a tab
    // is unreachable from any view without tabs — but the brink.toml
    // interface made it a surface you consult, and taking over the editor
    // cost you the file you were reading for something you leave in
    // seconds.
    run: () => openSettings(DEFAULT_SETTINGS_SECTION),
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

// ── Debugging persistence ───────────────────────────────────────────
//
// W1/#3294 (ruled 2026-08-29, "debug info on by default"): every studio
// compile emits the DebugInfo section unless the author opts out here.
// Same lenient-loader posture as the other keys; only an explicit,
// persisted `false` opts out — absent, garbage, and anything else land on
// the ruled default.

export const DEBUG_STORAGE_KEY = "brink-studio.debug.v1";

export interface DebugSettings {
  /** Emit the `DebugInfo` section in studio compiles. Default ON — the
   *  opt-out exists for authors who measure a compile cost they don't
   *  want to pay; with it off, breakpoints refuse to bind and stepping
   *  reports no source position (spec F1's honest degradation). */
  emitDebugInfo: boolean;
}

const DEFAULT_DEBUG: DebugSettings = { emitDebugInfo: true };

/** Load persisted debugging settings. Never throws; defaults on garbage. */
export function loadDebugSettings(storage: Pick<Storage, "getItem">): DebugSettings {
  let raw: string | null;
  try {
    raw = storage.getItem(DEBUG_STORAGE_KEY);
  } catch {
    return DEFAULT_DEBUG;
  }
  if (raw === null || raw === "") return DEFAULT_DEBUG;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_DEBUG;
  }
  const obj = parsed as { emitDebugInfo?: unknown } | null;
  // Default ON: only an explicit false (a persisted opt-out) disables it.
  return { emitDebugInfo: obj?.emitDebugInfo !== false };
}

/** Persist debugging settings. Storage failures degrade to in-session. */
export function saveDebugSettings(
  storage: Pick<Storage, "setItem">,
  settings: DebugSettings,
): void {
  try {
    storage.setItem(DEBUG_STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Quota/denied storage — the setting still applies for this session.
  }
}

// ── Player persistence (W7/#3300 F13) ───────────────────────────────

export const PLAYER_STORAGE_KEY = "brink-studio.player.v1";

export interface PlayerSettings {
  /** Paced auto-reveal cadence in ms (RULED: paced by default, ~150 ms).
   *  0 = "all at once" (one batch per reveal). */
  pacedRevealMs: number;
  /** Player prose size in px (W13/#3306); 0 = follow the app scale. */
  fontSize: number;
  /** Default target for NEW saves (W14/#3307); both stores stay
   * visible regardless. */
  saveLocation: "local" | "project";
}

const DEFAULT_PLAYER: PlayerSettings = {
  pacedRevealMs: 150,
  fontSize: 0,
  saveLocation: "local",
};

/** Load persisted player settings. Never throws; defaults on garbage. */
export function loadPlayerSettings(storage: Pick<Storage, "getItem">): PlayerSettings {
  let raw: string | null;
  try {
    raw = storage.getItem(PLAYER_STORAGE_KEY);
  } catch {
    return DEFAULT_PLAYER;
  }
  if (raw === null || raw === "") return DEFAULT_PLAYER;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_PLAYER;
  }
  const obj = parsed as {
    pacedRevealMs?: unknown;
    fontSize?: unknown;
    saveLocation?: unknown;
  } | null;
  const ms = obj?.pacedRevealMs;
  const px = obj?.fontSize;
  return {
    pacedRevealMs:
      typeof ms === "number" && Number.isFinite(ms) && ms >= 0
        ? Math.round(ms)
        : DEFAULT_PLAYER.pacedRevealMs,
    fontSize:
      typeof px === "number" && Number.isFinite(px) && px >= 0
        ? Math.round(px)
        : DEFAULT_PLAYER.fontSize,
    saveLocation: obj?.saveLocation === "project" ? "project" : "local",
  };
}

/** Persist player settings. Storage failures degrade to in-session. */
export function savePlayerSettings(
  storage: Pick<Storage, "setItem">,
  settings: PlayerSettings,
): void {
  try {
    storage.setItem(PLAYER_STORAGE_KEY, JSON.stringify(settings));
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
export function EditorViewSection() {
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
      <SettingsGroup title="View">
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
      </SettingsGroup>
    </section>
  );
}

export function ThemeSection() {
  const { themes } = useShell();
  const current = useThemeId();

  return (
    <section className="settings-section">
      <SettingsGroup title="Theme">
        <p className="settings-group-hint">
          Applies immediately. Each tile is the real theme &mdash; the same token
          cascade the editor resolves.
        </p>
        <ThemePicker
          themes={themes.list()}
          current={current}
          onSelect={(id) => void themes.select(id)}
        />
      </SettingsGroup>
    </section>
  );
}

export function KeymapSection() {
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
    <details className="settings-section settings-escape-hatch">
      <summary>Edit as JSON</summary>
      <p className="settings-section-hint">
        JSON mapping a command id to a keybinding ({'"Mod-K"'}), an array of
        keybindings, or <code>null</code> to unbind. Overrides replace the
        command{"'"}s default bindings and take effect on Apply. The table
        above writes this file; edit it directly for anything the table
        cannot express.
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
    </details>
  );
}

export function DiagnosticsSection() {
  const externalCheck = useStudioStore((s) => s.externalCheck);
  const setExternalCheck = useStudioStore((s) => s.setExternalCheck);
  const selectId = useId();

  const onChange = (level: ExternalCheckLevel): void => {
    setExternalCheck(level);
    saveDiagnosticsSettings(window.localStorage, { externalCheck: level });
  };

  return (
    <section className="settings-section">
      <SettingsRow
        htmlFor={selectId}
        title="External function checking"
        description="Severity of external-function checks against a registered host manifest. Recompiles on change."
      >
        <select
          id={selectId}
          className="settings-select"
          value={externalCheck}
          onChange={(event) => onChange(event.target.value as ExternalCheckLevel)}
        >
          <option value="error">Error</option>
          <option value="off">Off</option>
        </select>
      </SettingsRow>
    </section>
  );
}

export function DebuggingSection() {
  const debugInfoEnabled = useStudioStore((s) => s.debugInfoEnabled);
  const setDebugInfoEnabled = useStudioStore((s) => s.setDebugInfoEnabled);
  const debugInfoId = useId();

  const onChange = (on: boolean): void => {
    // The store action pushes the flag to the session and recompiles (only
    // codegen re-runs; diagnostics stay memoized), so the change takes
    // effect on the very next bytes the Player runs.
    setDebugInfoEnabled(on);
    saveDebugSettings(window.localStorage, { emitDebugInfo: on });
  };

  return (
    <div className="settings-section">
      <SettingsRow
        title="Emit debug info in studio compiles"
        description="On by default: breakpoints bind and stepping resolves to source with no restart. Turn off only if compile time on a large project becomes noticeable — the debugger then degrades honestly (breakpoints refuse to bind rather than lying)."
        htmlFor={debugInfoId}
      >
        <SettingsToggle id={debugInfoId} checked={debugInfoEnabled} onChange={onChange} />
      </SettingsRow>
    </div>
  );
}

/**
 * Conventions preview (#3391): paste a few lines and see how the
 * PROJECT's dialect reads them — the source-side classification the
 * editor applies, and, treating the same lines as your engine's emitted
 * text, the runs the Player would fold them into. The place an author
 * discovers a missing run-break rule before their engine does.
 */
export function ConventionsSection() {
  const dialect = useStudioStore((s) => s.projectDialect);
  const [sample, setSample] = useState(
    "@ALICE: <>\nA line with the cue attached.\nA second line, still Alice.\n> An action paragraph.\n",
  );
  const preview = useMemo(() => {
    if (dialect === null) return null;
    const parser = new DialectParser(dialect);
    const source = parser.parseSource(sample).map((l) => l.kind ?? "narrative");
    const lines = sample.split("\n").filter((t, i, all) => !(i === all.length - 1 && t === ""));
    const emitted = lines.map((text) => ({ segments: parser.parseEmitted(text) }));
    const runs = runsOf(emitted, dialect);
    return { source, lines, runs };
  }, [dialect, sample]);
  return (
    <div className="settings-section">
      <SettingsRow
        title="Project dialect"
        description={
          dialect === null
            ? "This project declares no [dialogue] in brink.toml — lines print as plain text. Add a preset (e.g. at-cue) to opt in."
            : `Resolved from brink.toml: ${dialect.name} — ${(dialect.elements ?? []).map((e) => e.kind).join(", ")}`
        }
      >
        <span className="settings-value sv-mono">{dialect === null ? "none" : dialect.name}</span>
      </SettingsRow>
      <SettingsRow
        title="Preview"
        description="Paste sample lines. Left: how the editor classifies them as SOURCE. Right: treating the same lines as your engine's EMITTED text, the runs the Player folds them into (who is speaking)."
      >
        <textarea
          className="settings-preview-input sv-mono"
          rows={6}
          value={sample}
          onChange={(e) => setSample(e.target.value)}
          spellCheck={false}
        />
      </SettingsRow>
      {preview && (
        <div className="settings-preview-grid">
          <div>
            <div className="settings-group-label">as source</div>
            {preview.lines.map((text, i) => (
              <div key={i} className="settings-preview-row">
                <code className="sv-mono">{preview.source[i] ?? "narrative"}</code>
                <span>{text}</span>
              </div>
            ))}
          </div>
          <div>
            <div className="settings-group-label">as emitted → Player</div>
            {preview.runs.map((run, i) => (
              <div key={i} className="settings-preview-run">
                <code className="sv-mono">
                  {run.kind ?? "narrative"}
                  {run.attrs.speaker ? ` · ${run.attrs.speaker}` : ""}
                </code>
                {run.lines.map((li) => (
                  <span key={li}>{preview.lines[li]}</span>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

export function PlayerSection() {
  const pacedMs = useStudioStore((s) => s.sessionPacedMs);
  const setSessionPaced = useStudioStore((s) => s.setSessionPaced);
  const playerFontSize = useStudioStore((s) => s.playerFontSize);
  const setPlayerFontSize = useStudioStore((s) => s.setPlayerFontSize);
  const saveLocation = useStudioStore((s) => s.saveLocationDefault);
  const setSaveLocationDefault = useStudioStore((s) => s.setSaveLocationDefault);
  const pacedId = useId();
  const saveLocId = useId();

  const persist = (over: Partial<PlayerSettings>): void => {
    savePlayerSettings(window.localStorage, {
      pacedRevealMs: pacedMs,
      fontSize: playerFontSize,
      saveLocation,
      ...over,
    });
  };
  const onModeChange = (paced: boolean): void => {
    const ms = paced ? 150 : 0;
    setSessionPaced(ms);
    persist({ pacedRevealMs: ms });
  };
  const onFontChange = (px: number): void => {
    setPlayerFontSize(px);
    persist({ fontSize: px });
  };
  const onSaveLocChange = (loc: "local" | "project"): void => {
    setSaveLocationDefault(loc);
    persist({ saveLocation: loc });
  };

  return (
    <div className="settings-section">
      <SettingsRow
        title="Auto reveal: paced"
        description="With the fast-forward toggle on, a reveal delivers the run one line at a time in rapid succession (paced), instead of dropping the whole chunk at once. Pausing or hitting a breakpoint stops the run instantly."
        htmlFor={pacedId}
      >
        <SettingsToggle id={pacedId} checked={pacedMs > 0} onChange={onModeChange} />
      </SettingsRow>
      <SettingsRow
        title="Player font size"
        description="10–32px; 0 follows the app type scale. Sizes the Player's prose only — the reading surface, not the studio chrome (W13)."
      >
        <SettingsStepper
          value={playerFontSize}
          min={0}
          max={32}
          label="player font size"
          suffix="px"
          onChange={onFontChange}
        />
      </SettingsRow>
      <SettingsRow
        htmlFor={saveLocId}
        title="Default save location"
        description="Where NEW saves land (W14) — both stores always show in the launcher. 'Project' saves are shareable with the project; 'This computer' stays local."
      >
        <select
          id={saveLocId}
          value={saveLocation}
          onChange={(e) => onSaveLocChange(e.target.value as "local" | "project")}
        >
          <option value="local">This computer</option>
          <option value="project">Project</option>
        </select>
      </SettingsRow>
    </div>
  );
}

export function EditorSection() {
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
      <SettingsGroup title="Arguments">
        <SettingsRow
          htmlFor={selectId}
          title="Argument-form glyph"
          description="When the inline glyph appears. The hover card's “edit arguments” action and Mod-Shift-A work regardless."
        >
          <select
            id={selectId}
            className="settings-select"
            value={formGlyph}
            onChange={(event) => onGlyphChange(event.target.value as FormGlyphMode)}
          >
            <option value="off">Off</option>
            <option value="hover">On line hover</option>
            <option value="inline">Always visible</option>
          </select>
        </SettingsRow>
        <SettingsRow
          htmlFor={autoId}
          title="Open the form on completion"
          description="Accepting a function completion opens its argument form."
        >
          <SettingsToggle id={autoId} checked={autoOpenForm} onChange={onAutoChange} />
        </SettingsRow>
      </SettingsGroup>

      <SettingsGroup title="Appearance">
        <SettingsRow
          htmlFor={guttersId}
          title="Show gutters"
          description="Line numbers, structure rails, and the fold and play markers."
        >
          <SettingsToggle id={guttersId} checked={showGutters} onChange={onGuttersChange} />
        </SettingsRow>
        <SettingsRow
          title="Editor font size"
          description={`${MIN_EDITOR_FONT_SIZE}–${MAX_EDITOR_FONT_SIZE}px, default ${DEFAULT_EDITOR_FONT_SIZE}. Mod-= / Mod-- / Mod-0 while editing do the same.`}
        >
          <SettingsStepper
            value={fontSize}
            min={MIN_EDITOR_FONT_SIZE}
            max={MAX_EDITOR_FONT_SIZE}
            label="editor font size"
            suffix="px"
            onChange={onFontSizeChange}
          />
        </SettingsRow>
        <SettingsRow
          title="App font size"
          description={`${MIN_APP_FONT_SIZE}–${MAX_APP_FONT_SIZE}px, default ${DEFAULT_APP_FONT_SIZE}. Sizes the studio's own chrome.`}
        >
          <SettingsStepper
            value={appFontSize}
            min={MIN_APP_FONT_SIZE}
            max={MAX_APP_FONT_SIZE}
            label="app font size"
            suffix="px"
            onChange={onAppFontSizeChange}
          />
        </SettingsRow>
      </SettingsGroup>
    </section>
  );
}

export function SettingsDocument({ groupId }: DocumentViewProps) {
  return (
    <div className="settings-doc">
      <div className="settings-doc-inner">
        <ProjectSection groupId={groupId} />
        <ThemeSection />
        <EditorViewSection />
        <EditorSection />
        <KeymapSection />
        <DiagnosticsSection />
        <LintSettings />
      </div>
    </div>
  );
}
