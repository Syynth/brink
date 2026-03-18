import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";

export const brinkTheme: Extension = EditorView.theme({
  "&": {
    height: "100%",
    backgroundColor: "var(--brink-bg, #1e1e2e)",
    color: "var(--brink-fg, #cdd6f4)",
  },
  ".cm-scroller": {
    overflow: "auto",
    fontFamily: '"Inter", "SF Pro Text", system-ui, -apple-system, sans-serif',
    fontSize: "15px",
    lineHeight: "1.6",
  },
  ".cm-gutters": {
    backgroundColor: "var(--brink-bg, #1e1e2e)",
    borderRight: "1px solid var(--brink-border, #45475a)",
    color: "var(--brink-fg-dim, #6c7086)",
  },
  ".cm-activeLineGutter, &.cm-focused .cm-activeLineGutter": {
    backgroundColor: "var(--brink-bg-surface, #252536)",
  },
  ".cm-activeLine": {
    backgroundColor: "var(--brink-bg-surface, #252536)",
  },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
    backgroundColor: "#44475a !important",
  },
  ".cm-cursor": {
    borderLeftColor: "var(--brink-accent, #89b4fa)",
  },
  ".cm-content": {
    caretColor: "var(--brink-accent, #89b4fa)",
  },
});
