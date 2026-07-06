import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";

// Colors reference only the semantic --bs-* tokens (studio-shell-spec §7.4);
// the values come from the active theme's CSS. No hardcoded fallbacks — the
// editor always mounts under a .brink-studio root that defines the tokens.

// Weave-depth indent (#414): `screenplay.ts`'s line-decoration pass emits a
// `data-depth="N"` attribute on choice/gather lines at depth > 1 — no inline
// `style` — and this is the *only* place that turns it into a look. Plain CSS
// attribute selectors can't do arithmetic on the attribute's value (`attr()`
// in `calc()` is unsupported on our Chromium 88 floor, #276), so we generate
// one rule per depth up to a generous cap; deeper weaves (vanishingly rare in
// practice) simply stop gaining additional indent past the cap instead of
// losing it entirely.
const DEPTH_INDENT_EM = 2;
const MAX_INDENT_DEPTH = 32;

function depthIndentRules(): Record<string, { paddingLeft: string }> {
  const rules: Record<string, { paddingLeft: string }> = {};
  // Fallback first (source order, not specificity, breaks the tie against the
  // exact-depth rules below — plain attribute-presence and attribute-value
  // selectors have equal specificity): any depth beyond the cap keeps the
  // cap's indent rather than reverting to zero.
  rules[".cm-line[data-depth]"] = {
    paddingLeft: `${(MAX_INDENT_DEPTH - 1) * DEPTH_INDENT_EM}em`,
  };
  for (let depth = 2; depth <= MAX_INDENT_DEPTH; depth++) {
    rules[`.cm-line[data-depth="${depth}"]`] = {
      paddingLeft: `${(depth - 1) * DEPTH_INDENT_EM}em`,
    };
  }
  return rules;
}

export const brinkTheme: Extension = [
  EditorView.theme({
    // Standalone diverts (#414): "screenplay transition" look — right-align
    // — keyed off the `brink-divert-standalone` class the line-decoration
    // pass adds (no inline style; headless hosts restyle or ignore freely).
    ".brink-divert-standalone": {
      textAlign: "right",
    },
    ...depthIndentRules(),
    "&": {
      height: "100%",
      backgroundColor: "var(--bs-editor-bg)",
      color: "var(--bs-fg)",
    },
    ".cm-scroller": {
      overflow: "auto",
      fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", monospace',
      fontSize: "14px",
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
