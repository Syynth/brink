/**
 * Hanging indent for soft-wrapped lines (maintainer follow-up to the
 * 2026-08-23 "literal whitespace" ruling): a wrapped line's continuation
 * rows align EVEN with the first row's text start — not flush-left
 * (which ran the indent guides straight through the wrapped text), and
 * not Inky's extra continuation padding (explicitly unwanted).
 *
 * The classic CM6 recipe: each line with leading whitespace carries a
 * `--line-indent: <n>ch` custom property (the editor is monospace, so a
 * column is a `ch`; tabs advance to the next tab stop), and a baseTheme
 * rule pads the line by that much while pulling the FIRST row back by
 * the same amount — net zero for the first row, hanging alignment for
 * every wrapped row. The inline style carries ONLY the custom property,
 * the exact shape the #414 no-inline-styles audit admits (see
 * `structural-decoration-attrs.test.ts`).
 */

import { RangeSetBuilder } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";

/** Leading-whitespace width of `text` in columns: spaces are 1, a tab
 *  advances to the next `tabSize` stop (matching how the editor renders
 *  it). Exported for its unit test. */
export function indentColumns(text: string, tabSize: number): number {
  let n = 0;
  for (const ch of text) {
    if (ch === " ") n += 1;
    else if (ch === "\t") n += tabSize - (n % tabSize);
    else break;
  }
  return n;
}

function buildIndents(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const tabSize = view.state.tabSize;
  for (const { from, to } of view.visibleRanges) {
    let pos = from;
    while (pos <= to) {
      const line = view.state.doc.lineAt(pos);
      const cols = indentColumns(line.text, tabSize);
      if (cols > 0) {
        builder.add(
          line.from,
          line.from,
          Decoration.line({ attributes: { style: `--line-indent: ${cols}ch` } }),
        );
      }
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

const hangingIndentPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = buildIndents(view);
    }
    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildIndents(update.view);
      }
    }
  },
  { decorations: (v) => v.decorations },
);

// Scoped to wrapping editors; inert (net-zero geometry) elsewhere anyway.
// 4px matches @codemirror/view's base `.cm-line` padding-left, so the
// first row sits exactly where it does without this extension.
const hangingIndentTheme = EditorView.baseTheme({
  ".cm-content.cm-lineWrapping .cm-line": {
    paddingLeft: "calc(4px + var(--line-indent, 0ch))",
    textIndent: "calc(-1 * var(--line-indent, 0ch))",
  },
});

/** The hanging-indent extension: always on in `brinkStudio` (it is part
 *  of the literal-whitespace presentation — the file's indentation is
 *  the layout, so wrapping honors it). */
export function hangingIndent() {
  return [hangingIndentPlugin, hangingIndentTheme];
}
