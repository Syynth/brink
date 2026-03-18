import { type Extension } from "@codemirror/state";
import { foldService } from "@codemirror/language";
import type { FoldRange } from "../wasm.js";

export interface FoldingOptions {
  getFoldingRanges: (source: string) => FoldRange[];
}

export function foldingExtension(options: FoldingOptions): Extension {
  let cachedSource = "";
  let cachedRanges: FoldRange[] = [];

  return foldService.of((state, lineStart, lineEnd) => {
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
        return { from: line.to, to: endLine.to };
      }
    }

    return null;
  });
}
