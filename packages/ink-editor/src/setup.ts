/**
 * The editor's base extension set — `codemirror`'s `basicSetup` list copied
 * per that package's own instruction ("once you decide you want to configure
 * your editor more precisely, you take this package's source … and adjust
 * it"), so the fold gutter can be ours instead of the stock text-glyph one.
 *
 * The stock `foldGutter()` renders `⌄`/`›` as bare text: the glyph swap
 * changes metrics between the open and collapsed states (the marker
 * visibly shifts when a region collapses), and the markers sit awkwardly
 * against the line numbers. This setup's fold gutter renders ONE chevron
 * SVG in a fixed slot for both states — collapsed is the same glyph
 * rotated back via CSS, so nothing moves — and follows the familiar
 * editor convention: collapsed markers are always visible (accented,
 * since they hide content), open markers appear only while the pointer
 * is over the gutter.
 *
 * Everything else matches `basicSetup` exactly, in the same order.
 */

import {
  EditorView,
  crosshairCursor,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import { EditorState, type Extension } from "@codemirror/state";
import {
  bracketMatching,
  defaultHighlightStyle,
  foldGutter,
  foldKeymap,
  indentOnInput,
  syntaxHighlighting,
} from "@codemirror/language";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { highlightSelectionMatches, searchKeymap } from "@codemirror/search";
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
} from "@codemirror/autocomplete";
import { lintKeymap } from "@codemirror/lint";

/** One chevron for both fold states — rotation is CSS, so metrics never change. */
function foldMarkerDOM(open: boolean): HTMLElement {
  const el = document.createElement("span");
  el.className = "brink-fold-marker " + (open ? "brink-fold-open" : "brink-fold-closed");
  el.title = open ? "Fold" : "Unfold";
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 12 12");
  svg.setAttribute("width", "10");
  svg.setAttribute("height", "10");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", "M4.3 2.6 L8.1 6 L4.3 9.4");
  path.setAttribute("fill", "none");
  path.setAttribute("stroke", "currentColor");
  path.setAttribute("stroke-width", "1.6");
  path.setAttribute("stroke-linecap", "round");
  path.setAttribute("stroke-linejoin", "round");
  svg.appendChild(path);
  el.appendChild(svg);
  return el;
}

const foldGutterTheme = EditorView.baseTheme({
  ".cm-foldGutter .cm-gutterElement": {
    cursor: "pointer",
  },
  ".brink-fold-marker": {
    boxSizing: "border-box",
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    // Fixed slot both states share — `initialSpacer`-equivalent stability
    // comes from the marker itself never changing size.
    width: "14px",
    // First-ROW height, not full (possibly wrapped) line height — the
    // chevron top-aligns with the line number beside it.
    height: "1lh",
    color: "var(--bs-fg-muted, #888)",
  },
  ".brink-fold-marker svg": {
    transition: "transform 120ms ease",
  },
  // Open region: chevron points down (rotated), only shown while the
  // pointer is over the gutter — the numbers stay clean otherwise.
  ".brink-fold-open svg": {
    transform: "rotate(90deg)",
  },
  ".brink-fold-open": {
    opacity: "0",
  },
  ".cm-gutters:hover .brink-fold-open": {
    opacity: "0.7",
  },
  ".cm-gutters:hover .brink-fold-open:hover, .brink-fold-open:hover": {
    opacity: "1",
  },
  // Collapsed region: always visible and accented — it hides content.
  ".brink-fold-closed": {
    color: "var(--bs-accent, #3b82f6)",
    opacity: "0.9",
  },
  "@media (prefers-reduced-motion: reduce)": {
    ".brink-fold-marker svg": {
      transition: "none",
    },
  },
});

/**
 * Drop-in replacement for `codemirror`'s `basicSetup` (same members, same
 * order) with the brink fold gutter.
 */
export const brinkBasicSetup: Extension = [
  lineNumbers(),
  highlightActiveLineGutter(),
  highlightSpecialChars(),
  history(),
  foldGutter({ markerDOM: foldMarkerDOM }),
  foldGutterTheme,
  drawSelection(),
  dropCursor(),
  EditorState.allowMultipleSelections.of(true),
  indentOnInput(),
  syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
  bracketMatching(),
  closeBrackets(),
  autocompletion(),
  rectangularSelection(),
  crosshairCursor(),
  highlightActiveLine(),
  highlightSelectionMatches(),
  keymap.of([
    ...closeBracketsKeymap,
    ...defaultKeymap,
    ...searchKeymap,
    ...historyKeymap,
    ...foldKeymap,
    ...completionKeymap,
    ...lintKeymap,
  ]),
];
