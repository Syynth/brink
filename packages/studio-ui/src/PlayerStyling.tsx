/**
 * Settings → Player: Reading (font, line spacing, measure) and Reading
 * aids (provenance button, choice markers) — #3438, ruled 2026-09-02 ("we
 * should have some player styling features available in settings"). App
 * scope, persisted with the Player settings.
 *
 * Fonts: browsers cannot enumerate installed fonts, so the web build
 * offers a curated list of reading faces plus a family you type; the
 * desktop app supplies the machine's fonts through `hostFonts` (#3439).
 * Nothing is fetched — a name resolves to what the machine has, with a
 * fallback stack, and a missing face falls back per CSS.
 */

import { useId, useMemo, useState } from "react";
import { sanitizeFontFamily } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";
import { SettingsRow, SettingsStepper, SettingsToggle } from "./SettingsRow.js";
import { loadPlayerSettings, savePlayerSettings, type PlayerSettings } from "./SettingsDocument.js";

/** The curated reading faces (name → CSS family list). */
export const CURATED_FONTS: ReadonlyArray<{ name: string; family: string }> = [
  { name: "Georgia", family: "Georgia, 'Palatino Linotype', 'Book Antiqua', serif" },
  { name: "Iowan Old Style", family: "'Iowan Old Style', 'Palatino Linotype', Palatino, serif" },
  { name: "Charter", family: "Charter, 'Bitstream Charter', 'Sitka Text', Cambria, serif" },
  { name: "Palatino", family: "Palatino, 'Palatino Linotype', 'Book Antiqua', serif" },
  { name: "Literata", family: "Literata, Georgia, serif" },
  { name: "Courier Prime", family: "'Courier Prime', 'Courier New', Courier, monospace" },
  { name: "Public Sans", family: "'Public Sans', 'Helvetica Neue', Arial, sans-serif" },
  { name: "System serif", family: "ui-serif, Georgia, serif" },
  { name: "System sans", family: "system-ui, -apple-system, 'Segoe UI', sans-serif" },
  { name: "JetBrains Mono", family: "'JetBrains Mono', 'Fira Code', 'Cascadia Code', monospace" },
];

const SPECIMEN = "The lantern gutters. “Not even close.”";

function persist(over: Partial<PlayerSettings>): void {
  savePlayerSettings(window.localStorage, { ...loadPlayerSettings(window.localStorage), ...over });
}

export function PlayerReadingSection() {
  const fontFamily = useStudioStore((s) => s.playerFontFamily);
  const setPlayerFontFamily = useStudioStore((s) => s.setPlayerFontFamily);
  const lineHeight = useStudioStore((s) => s.playerLineHeight);
  const setPlayerLineHeight = useStudioStore((s) => s.setPlayerLineHeight);
  const measure = useStudioStore((s) => s.playerMeasure);
  const setPlayerMeasure = useStudioStore((s) => s.setPlayerMeasure);
  const hostFonts = useStudioStore((s) => s.hostFonts);
  const [filter, setFilter] = useState("");
  const [custom, setCustom] = useState("");
  const customId = useId();
  const filterId = useId();

  const list = useMemo(() => {
    const base =
      hostFonts === null
        ? CURATED_FONTS
        : hostFonts.map((name) => ({ name, family: `'${name.replace(/'/g, "")}'` }));
    const q = filter.trim().toLowerCase();
    return q === "" ? base : base.filter((f) => f.name.toLowerCase().includes(q));
  }, [hostFonts, filter]);

  const choose = (family: string): void => {
    setPlayerFontFamily(family);
    persist({ fontFamily: sanitizeFontFamily(family) });
  };
  const customInvalid = custom.trim() !== "" && sanitizeFontFamily(custom) === "";
  const current =
    fontFamily === "" ? "default" : (fontFamily.split(",")[0] ?? "").replace(/['"]/g, "");

  return (
    <div className="settings-section">
      <SettingsRow
        title="Font"
        description={
          hostFonts === null
            ? "The Player's reading face. A curated set on the web; type a family below to use one installed on this machine."
            : "The Player's reading face — every font on this machine."
        }
      >
        <span className="settings-value sv-mono">{current}</span>
      </SettingsRow>
      <div className="font-picker">
        {hostFonts !== null && (
          <input
            id={filterId}
            type="search"
            className="font-picker-filter"
            placeholder="Filter fonts"
            aria-label="Filter fonts"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        )}
        <div className="font-picker-list" role="listbox" aria-label="Reading font">
          <button
            type="button"
            role="option"
            aria-selected={fontFamily === ""}
            className={"font-picker-row" + (fontFamily === "" ? " on" : "")}
            onClick={() => choose("")}
          >
            <span className="font-picker-specimen">{SPECIMEN}</span>
            <span className="font-picker-name">Theme default</span>
          </button>
          {list.map((f) => (
            <button
              key={f.name}
              type="button"
              role="option"
              aria-selected={fontFamily === f.family}
              className={"font-picker-row" + (fontFamily === f.family ? " on" : "")}
              onClick={() => choose(f.family)}
            >
              <span className="font-picker-specimen" style={{ fontFamily: f.family }}>
                {SPECIMEN}
              </span>
              <span className="font-picker-name">{f.name}</span>
            </button>
          ))}
        </div>
        <div className="font-picker-custom">
          <label htmlFor={customId}>Custom family</label>
          <input
            id={customId}
            type="text"
            className="font-picker-input sv-mono"
            placeholder='"Iosevka Etoile", serif'
            value={custom}
            aria-invalid={customInvalid}
            onChange={(e) => setCustom(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !customInvalid && custom.trim() !== "") choose(custom.trim());
            }}
          />
          <button
            type="button"
            className="settings-apply"
            disabled={customInvalid || custom.trim() === ""}
            onClick={() => choose(custom.trim())}
          >
            Use
          </button>
        </div>
        {customInvalid && (
          <p className="settings-error">A font family is names, quotes and commas only.</p>
        )}
      </div>
      <SettingsRow
        title="Line spacing"
        description="Space between lines of prose, as a multiple of the font size (shown ×10). 0 follows the theme (1.8)."
      >
        <SettingsStepper
          value={lineHeight}
          min={0}
          max={22}
          label="line spacing"
          suffix=" /10"
          onChange={(v) => {
            const next = v > 0 && v < 12 ? (lineHeight === 0 ? 12 : 0) : v;
            setPlayerLineHeight(next);
            persist({ lineHeight: next });
          }}
        />
      </SettingsRow>
      <SettingsRow
        title="Measure"
        description="How wide a line of prose may run before it wraps, in characters. 0 follows the theme."
      >
        <SettingsStepper
          value={measure}
          min={0}
          max={96}
          label="measure"
          suffix=" ch"
          onChange={(v) => {
            const next = v > 0 && v < 48 ? (measure === 0 ? 48 : 0) : v;
            setPlayerMeasure(next);
            persist({ measure: next });
          }}
        />
      </SettingsRow>
    </div>
  );
}

export function PlayerReadingAidsSection() {
  const showProvenance = useStudioStore((s) => s.showProvenance);
  const setShowProvenance = useStudioStore((s) => s.setShowProvenance);
  const showChoiceMarkers = useStudioStore((s) => s.showChoiceMarkers);
  const setShowChoiceMarkers = useStudioStore((s) => s.setShowChoiceMarkers);
  const provId = useId();
  const marksId = useId();
  return (
    <div className="settings-section">
      <SettingsRow
        title="Show where a line came from"
        description="The go-to-source button beside a line you hover; ⌘-click a line opens it in the editor either way."
        htmlFor={provId}
      >
        <SettingsToggle
          id={provId}
          checked={showProvenance}
          onChange={(on) => {
            setShowProvenance(on);
            persist({ showProvenance: on });
          }}
        />
      </SettingsRow>
      <SettingsRow
        title="Choice markers"
        description="Show * or + beside a choice, matching how it was written — on the choices offered and on the one you took."
        htmlFor={marksId}
      >
        <SettingsToggle
          id={marksId}
          checked={showChoiceMarkers}
          onChange={(on) => {
            setShowChoiceMarkers(on);
            persist({ showChoiceMarkers: on });
          }}
        />
      </SettingsRow>
    </div>
  );
}
