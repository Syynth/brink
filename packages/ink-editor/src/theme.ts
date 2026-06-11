import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";

// Colors reference only the semantic --bs-* tokens (studio-shell-spec §7.4);
// the values come from the active theme's CSS. No hardcoded fallbacks — the
// editor always mounts under a .brink-studio root that defines the tokens.
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
      backgroundColor: "color-mix(in srgb, var(--bs-surface-bg) 60%, transparent)",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      backgroundColor:
        "color-mix(in srgb, var(--bs-accent) 30%, transparent) !important",
    },
    ".cm-cursor": {
      borderLeftColor: "var(--bs-accent)",
    },
    ".cm-content": {
      caretColor: "var(--bs-accent)",
    },
  }),
];
