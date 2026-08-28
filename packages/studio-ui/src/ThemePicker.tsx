/**
 * The theme picker — tiles that show the theme, not its name (#3174).
 *
 * A theme's name tells you nothing about what it looks like. "Manuscript"
 * and "Inky Dark" are choices you make by looking, which is why every IDE
 * installer picks themes from a wall of previews rather than a list; a
 * dropdown made you apply each one in turn to see it, and applying a theme
 * repaints the whole studio around you.
 *
 * **The preview is the real theme, not a picture of it.** Themes are token
 * sheets scoped `.brink-studio[data-theme="<id>"]` with mocha as the bare
 * default underneath, so a tile that carries BOTH the class and the
 * attribute resolves the genuine cascade for that theme inside itself. It
 * follows that a token that changes, or a theme that is added, shows up
 * here for free — the alternative (a palette copied into this file) is a
 * second source of truth that drifts the first time a colour is tuned, and
 * silently: a stale swatch still renders.
 *
 * For the same reason the snippet is marked up with the editor's OWN
 * `.tok-*` classes rather than inline colours. Those classes carry the
 * per-role fallbacks (a marker falls back to the operator colour in a
 * theme that doesn't split them), so a tile shows exactly what the editor
 * would show — including for a theme that defines none of the role tokens.
 */

import { useId } from "react";
import type { ThemeDescriptor } from "@brink/studio-shell";

/**
 * The snippet every tile renders.
 *
 * Chosen to exercise the roles a theme actually decides between, in the
 * fewest lines: a definition, prose (the one true foreground), a cue, the
 * structure markers, and a divert into a halt word. A snippet of pure
 * prose would make every theme look alike; one full of machinery would
 * misrepresent what an author spends the day looking at.
 */
const PREVIEW_LINES: { cls?: string; text: string }[][] = [
  [{ cls: "tok-keyword", text: "= " }, { cls: "tok-namespace", text: "haggle" }],
  [{ cls: "tok-comment", text: "// the lantern scene" }],
  [{ text: "KID" }],
  [{ text: "How much for the lantern?" }],
  [
    { cls: "tok-marker", text: "* [" },
    { text: "Offer five" },
    { cls: "tok-marker", text: "]" },
  ],
  [
    { cls: "tok-divert", text: "  -> " },
    { cls: "tok-halt", text: "DONE" },
  ],
];

export function ThemePicker({
  themes,
  current,
  onSelect,
}: {
  themes: ThemeDescriptor[];
  current: string;
  onSelect: (id: string) => void;
}) {
  // Unique per mounted picker: same-name radios across two mounted copies
  // (the singleton can still be split-duplicated) uncheck each other at the
  // DOM level.
  const groupName = useId();

  return (
    <div className="settings-theme-grid" role="radiogroup" aria-label="Theme">
      {themes.map((theme) => (
        <label
          key={theme.id}
          className={"settings-theme-tile" + (theme.id === current ? " active" : "")}
        >
          {/* A real radio, visually hidden rather than replaced: the group
              keeps arrow-key navigation, the focus ring, and the label
              association that a div with role="radio" would have to
              reimplement. */}
          <input
            type="radio"
            name={groupName}
            value={theme.id}
            checked={theme.id === current}
            onChange={() => onSelect(theme.id)}
          />
          {/* BOTH the class and the attribute — that pair IS the theme
              cascade (mocha's bare-class base, then this theme's
              overrides). `aria-hidden` because the snippet is decoration:
              the accessible name is the label below. */}
          <span className="brink-studio settings-theme-preview" data-theme={theme.id} aria-hidden>
            {PREVIEW_LINES.map((line, i) => (
              // eslint-disable-next-line react/no-array-index-key -- static, never reordered
              <span key={i} className="settings-theme-line">
                {line.map((run, j) => (
                  // eslint-disable-next-line react/no-array-index-key -- static, never reordered
                  <span key={j} className={run.cls}>
                    {run.text}
                  </span>
                ))}
              </span>
            ))}
          </span>
          <span className="settings-theme-name">{theme.label}</span>
        </label>
      ))}
    </div>
  );
}
