import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";

/** The editor's shipped text size, and the reset target for the zoom
 *  commands (beta feedback 2026-08-25: the editor gets its own size knob,
 *  separate from any app-wide sizing). */
export const DEFAULT_EDITOR_FONT_SIZE = 14;
/** Clamp bounds: below 8 the gutter collides with itself; above 32 a single
 *  line stops fitting a pane at any sane width. */
export const MIN_EDITOR_FONT_SIZE = 8;
export const MAX_EDITOR_FONT_SIZE = 32;

/**
 * Clamp + round an arbitrary value (or garbage from persisted settings) to
 * a usable editor size. One definition, shared by the store's setters and
 * the settings parser, so they can never disagree.
 */
export function clampEditorFontSize(value: unknown): number {
  const n = typeof value === "number" && Number.isFinite(value) ? Math.round(value) : NaN;
  if (Number.isNaN(n)) return DEFAULT_EDITOR_FONT_SIZE;
  return Math.min(MAX_EDITOR_FONT_SIZE, Math.max(MIN_EDITOR_FONT_SIZE, n));
}

// Colors reference only the semantic --bs-* tokens (studio-shell-spec §7.4);
// the values come from the active theme's CSS. No hardcoded fallbacks — the
// editor always mounts under a .brink-studio root that defines the tokens.

// Literal-whitespace presentation (ruled 2026-08-23, docs/decision-log.md
// "Editor layout: literal whitespace, Inky parity"): the theme imposes NO
// layout of its own — no weave-depth indent from `data-depth`, no
// right-aligned "transition" diverts. The classification CONTRACT is
// untouched: the line-decoration pass still emits `data-depth="N"` and
// `brink-divert-standalone` (plus every element class), so an embedder
// that wants a screenplay look adds its own CSS over those hooks — this
// file just no longer ships one.
export const brinkTheme: Extension = [
  EditorView.theme({
    "&": {
      height: "100%",
      backgroundColor: "var(--bs-editor-bg)",
      color: "var(--bs-fg)",
    },
    ".cm-scroller": {
      overflow: "auto",
      fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", monospace',
      fontSize: "var(--bs-editor-font-size, 14px)",
      lineHeight: "1.6",
    },
    ".cm-gutters": {
      backgroundColor: "var(--bs-editor-bg)",
      borderRight: "1px solid var(--bs-border)",
      color: "var(--bs-fg-muted)",
    },
    ".cm-activeLineGutter, &.cm-focused .cm-activeLineGutter": {
      backgroundColor: "var(--bs-surface-bg)",
    },
    ".cm-activeLine": {
      // color-mix() is unavailable on Chromium 88 (RMMZ/NW.js) — the theme
      // provides the precomputed translucent token; hosts that define only
      // base tokens degrade to the opaque surface (#276).
      backgroundColor: "var(--bs-active-line-bg, var(--bs-surface-bg))",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      // The selection layer sits behind the text, so a solid token + layer
      // opacity composites like a 30%-alpha fill without color-mix(), which
      // Chromium 88 drops — making selection invisible (#276).
      backgroundColor: "var(--bs-accent) !important",
      opacity: "0.3",
    },
    ".cm-cursor": {
      borderLeftColor: "var(--bs-accent)",
    },
    ".cm-content": {
      caretColor: "var(--bs-accent)",
    },
  }),
];
