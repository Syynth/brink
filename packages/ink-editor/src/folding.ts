import { Prec, type Extension, type EditorState } from "@codemirror/state";
import { foldService, codeFolding } from "@codemirror/language";
import type { FoldRange } from "@brink/wasm-types";

export interface FoldingOptions {
  getFoldingRanges: (source: string) => FoldRange[];
}

/** The CM6 fold `{from, to}` a `FoldRange` maps to, plus the range it came
 *  from, so the placeholder can read the Rust-supplied `collapsed_text`. */
interface ResolvedFold {
  from: number;
  to: number;
  range: FoldRange;
}

export function foldingExtension(options: FoldingOptions): Extension {
  let cachedSource = "";
  let cachedRanges: FoldRange[] = [];

  const rangesFor = (state: EditorState): FoldRange[] => {
    const source = state.doc.toString();
    // Cache ranges per source to avoid recomputing on every fold query.
    if (source !== cachedSource) {
      try {
        cachedRanges = options.getFoldingRanges(source);
      } catch {
        cachedRanges = [];
      }
      cachedSource = source;
    }
    return cachedRanges;
  };

  const service = foldService.of((state, lineStart, _lineEnd) => {
    const line = state.doc.lineAt(lineStart);
    const lineNum = line.number - 1; // 0-indexed

    for (const range of rangesFor(state)) {
      if (range.start_line === lineNum) {
        const resolved = resolveFold(state, range);
        if (resolved) return { from: resolved.from, to: resolved.to };
      }
    }

    return null;
  });

  return [service, Prec.high(codeFolding(placeholderConfig(rangesFor)))];
}

/** Resolve a `FoldRange` to the exact CM6 `{from, to}` its fold spans. Shared
 *  by the fold service and the placeholder so they agree on bounds. */
function resolveFold(state: EditorState, range: FoldRange): ResolvedFold | null {
  if (range.start_line < 0 || range.start_line >= state.doc.lines) return null;
  const line = state.doc.line(range.start_line + 1); // 1-indexed
  const endLine = state.doc.line(Math.min(range.end_line + 1, state.doc.lines));
  // Declaration folds (docs + header + body) hide the whole region —
  // including the anchor line — and render a header placeholder. Others fold
  // from the end of the anchor line, keeping that line visible.
  const from = range.from_line_start ? line.from : line.to;
  return { from, to: endLine.to, range };
}

/** The prepared placeholder value carried from `preparePlaceholder` to
 *  `placeholderDOM`. A discriminated shape (not a bare string) so the verbatim
 *  Rust `collapsed_text` of the INCLUDE-block fold can never be confused with a
 *  re-derived declaration header. */
export type FoldPlaceholder =
  | { kind: "collapsed"; text: string } // Rust `collapsed_text`, rendered verbatim.
  | { kind: "decl"; header: string | null }; // whole-declaration header (or none).

/** True when a `FoldRange` is the leading INCLUDE-block fold, i.e. the one fold
 *  whose Rust `collapsed_text` is a human-readable placeholder we render
 *  verbatim (e.g. `INCLUDE … (3 files)`).
 *
 *  Conditional/sequence folds also carry a `collapsed_text`, but it is the
 *  internal `{...}` sentinel the Rust core uses to trigger brace-span
 *  extension — never a display label. Those must fall through to the default
 *  placeholder, not be styled/announced as INCLUDE blocks. So presence of
 *  `collapsed_text` alone is NOT a valid discriminator; the include block is
 *  identified by its `INCLUDE` prefix. */
function isIncludeBlockFold(fr: FoldRange): boolean {
  return typeof fr.collapsed_text === "string" && fr.collapsed_text.startsWith("INCLUDE");
}

/** Build the `codeFolding` placeholder config. `preparePlaceholder` first
 *  looks for a `FoldRange` whose resolved fold matches this fold and carries a
 *  Rust-supplied `collapsed_text` (the INCLUDE-block fold) and renders that
 *  verbatim. Otherwise it falls back to the whole-declaration header. */
export function placeholderConfig(rangesFor: (state: EditorState) => FoldRange[]) {
  return {
    preparePlaceholder(state: EditorState, range: { from: number; to: number }): FoldPlaceholder {
      // A FoldRange whose `collapsed_text` should render verbatim — the
      // INCLUDE-block fold specifically. Match on the resolved fold bounds so
      // the Rust text, not a doc-slice re-derivation, drives the placeholder.
      // Conditional/sequence folds also carry a `collapsed_text` (the internal
      // `{...}` sentinel), so gate on the INCLUDE-block discriminator, not mere
      // presence of `collapsed_text`, or those folds get mislabeled as INCLUDE
      // blocks.
      for (const fr of rangesFor(state)) {
        if (!isIncludeBlockFold(fr)) continue;
        const resolved = resolveFold(state, fr);
        if (resolved && resolved.from === range.from && resolved.to === range.to) {
          // Non-null: isIncludeBlockFold guarantees a string collapsed_text.
          return { kind: "collapsed", text: fr.collapsed_text as string };
        }
      }
      return { kind: "decl", header: prepareDeclPlaceholder(state, range) };
    },
    placeholderDOM(
      _view: unknown,
      onclick: (event: Event) => void,
      prepared: FoldPlaceholder,
    ): HTMLElement {
      if (prepared.kind === "collapsed") {
        return includePlaceholderDOM(onclick, prepared.text);
      }
      return declPlaceholderDOM(onclick, prepared.header);
    },
  };
}

/** Render the leading INCLUDE-block fold's Rust `collapsed_text` verbatim
 *  (e.g. `INCLUDE (3 files)`), with no declaration-header styling. */
function includePlaceholderDOM(onclick: (event: Event) => void, text: string): HTMLElement {
  const el = document.createElement("span");
  el.className = "brink-fold-include";
  const label = document.createElement("span");
  label.className = "brink-fold-include-label";
  label.textContent = text;
  el.appendChild(label);
  el.setAttribute("aria-label", `folded INCLUDE block: ${text}`);
  el.onclick = onclick;
  return el;
}

/** Render a whole-declaration fold (docs + header + body) as its hidden
 *  header line, so a collapsed knot still reads as `=== name === …`. */
function prepareDeclPlaceholder(
  state: EditorState,
  range: { from: number; to: number },
): string | null {
  // Only whole-line folds qualify — body folds anchor at the end of the
  // header line, which is never a line start.
  if (state.doc.lineAt(range.from).from !== range.from) return null;
  for (const line of state.sliceDoc(range.from, range.to).split("\n")) {
    const trimmed = line.trim();
    if (trimmed.startsWith("///")) continue;
    return trimmed.startsWith("=") ? trimmed : null;
  }
  return null;
}

function declPlaceholderDOM(onclick: (event: Event) => void, prepared: string | null): HTMLElement {
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
}
