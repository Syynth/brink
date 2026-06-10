import { Prec, type Extension, type EditorState } from "@codemirror/state";
import { foldService, codeFolding } from "@codemirror/language";
import type { FoldRange } from "@brink/wasm-types";

export interface FoldingOptions {
  getFoldingRanges: (source: string) => FoldRange[];
}

export function foldingExtension(options: FoldingOptions): Extension {
  let cachedSource = "";
  let cachedRanges: FoldRange[] = [];

  const service = foldService.of((state, lineStart, _lineEnd) => {
    const source = state.doc.toString();

    // Cache ranges per source to avoid recomputing on every fold query
    if (source !== cachedSource) {
      try {
        cachedRanges = options.getFoldingRanges(source);
      } catch {
        cachedRanges = [];
      }
      cachedSource = source;
    }

    const line = state.doc.lineAt(lineStart);
    const lineNum = line.number - 1; // 0-indexed

    for (const range of cachedRanges) {
      if (range.start_line === lineNum) {
        const endLine = state.doc.line(Math.min(range.end_line + 1, state.doc.lines));
        // Declaration folds (docs + header + body) hide the whole region —
        // including the anchor line — and render a header placeholder.
        const from = range.from_line_start ? line.from : line.to;
        return { from, to: endLine.to };
      }
    }

    return null;
  });

  return [service, Prec.high(codeFolding(declPlaceholderConfig))];
}

/// Render a whole-declaration fold (docs + header + body) as its hidden
/// header line, so a collapsed knot still reads as `=== name === …`.
const declPlaceholderConfig = {
  preparePlaceholder(state: EditorState, range: { from: number; to: number }): string | null {
    // Only whole-line folds qualify — body folds anchor at the end of the
    // header line, which is never a line start.
    if (state.doc.lineAt(range.from).from !== range.from) return null;
    for (const line of state.sliceDoc(range.from, range.to).split("\n")) {
      const trimmed = line.trim();
      if (trimmed.startsWith("///")) continue;
      return trimmed.startsWith("=") ? trimmed : null;
    }
    return null;
  },
  placeholderDOM(
    _view: unknown,
    onclick: (event: Event) => void,
    prepared: string | null,
  ): HTMLElement {
    const el = document.createElement("span");
    if (prepared) {
      el.className = "brink-fold-decl";
      const header = document.createElement("span");
      header.className = "brink-fold-decl-header";
      header.textContent = prepared;
      el.appendChild(header);
      el.appendChild(document.createTextNode(" ⋯"));
    } else {
      el.className = "cm-foldPlaceholder";
      el.textContent = "…";
    }
    el.setAttribute("aria-label", "folded code");
    el.onclick = onclick;
    return el;
  },
};
