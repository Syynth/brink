/**
 * Leading INCLUDE-block fold (#313 G).
 *
 * The brink-ide core emits a `FoldRange` over the leading run of two-or-more
 * INCLUDEs with a Rust-supplied `collapsed_text` ("INCLUDE … (N files)"). This
 * exercises the CM6 wiring: a real `EditorView` (jsdom) with `foldingExtension`
 * driven through `foldable` → `foldEffect` → rendered placeholder, asserting the
 * placeholder reads the Rust `collapsed_text` verbatim (not a doc-slice
 * re-derivation), that folding/unfolding round-trips, that an absent or
 * single-INCLUDE block does not fold, and that the decl-fold placeholder is
 * unchanged.
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { foldable, foldEffect, unfoldEffect, foldedRanges } from "@codemirror/language";
import type { FoldRange } from "@brink/wasm-types";
import { foldingExtension } from "@brink/ink-editor";

// Deterministic stand-in for the wasm `getFoldingRanges`, mirroring what
// brink-ide emits: an INCLUDE-block fold (N>=2) with `collapsed_text`, a
// whole-declaration knot fold with `from_line_start`, and — crucially —
// conditional/sequence folds carrying the internal `{...}` sentinel
// `collapsed_text`. The real core emits `collapsed_text: Some("{...}")` for
// every Conditional/Sequence/InlineConditional/InlineSequence fold (see
// crates/internal/brink-ide/src/folding.rs); this stub must reproduce that so
// the "only the INCLUDE block gets the include placeholder" guarantee is
// actually exercised.
function fakeRanges(source: string): FoldRange[] {
  const lines = source.split("\n");
  const ranges: FoldRange[] = [];

  const includeLines = lines
    .map((l, i) => (l.trimStart().startsWith("INCLUDE ") ? i : -1))
    .filter((i) => i >= 0);
  // Leading run only: every include line must be contiguous from the top.
  const leading: number[] = [];
  for (let i = 0; i < includeLines.length; i++) {
    if (includeLines[i] === i) leading.push(includeLines[i]);
    else break;
  }
  if (leading.length >= 2) {
    ranges.push({
      start_line: leading[0],
      end_line: leading[leading.length - 1],
      collapsed_text: `INCLUDE … (${leading.length} files)`,
    });
  }

  // A knot header + one body line → a whole-declaration fold.
  const knot = lines.findIndex((l) => l.startsWith("=="));
  if (knot >= 0 && knot + 1 < lines.length) {
    ranges.push({ start_line: knot, end_line: knot + 1, from_line_start: true });
  }

  // Multi-line conditional/sequence: a line whose trimmed text starts with `{`
  // opens a block that a later `}`-only line closes. The core emits a body fold
  // (from_line_start false) carrying the internal `{...}` sentinel
  // `collapsed_text`. Model that here so the placeholder path is exercised.
  const openIdx = lines.findIndex((l) => l.trimStart().startsWith("{"));
  if (openIdx >= 0) {
    const closeIdx = lines.findIndex((l, i) => i > openIdx && l.trim() === "}");
    if (closeIdx > openIdx) {
      ranges.push({
        start_line: openIdx,
        end_line: closeIdx,
        collapsed_text: "{...}",
      });
    }
  }

  return ranges;
}

let view: EditorView | undefined;

function mount(doc: string): EditorView {
  view = new EditorView({
    state: EditorState.create({
      doc,
      extensions: [foldingExtension({ getFoldingRanges: fakeRanges })],
    }),
    parent: document.body,
  });
  return view;
}

// Fold the region the fold service reports for the given 0-based line.
function foldLine(v: EditorView, line0: number): { from: number; to: number } | null {
  const line = v.state.doc.line(line0 + 1);
  const region = foldable(v.state, line.from, line.to);
  if (!region) return null;
  v.dispatch({ effects: foldEffect.of(region) });
  return region;
}

function placeholderText(v: EditorView): string | null {
  const el =
    v.dom.querySelector(".brink-fold-include-label") ??
    v.dom.querySelector(".brink-fold-decl-header") ??
    v.dom.querySelector(".cm-foldPlaceholder");
  return el?.textContent ?? null;
}

afterEach(() => {
  view?.destroy();
  view = undefined;
  document.body.innerHTML = "";
});

describe("leading INCLUDE-block fold", () => {
  const THREE = "INCLUDE a.ink\nINCLUDE b.ink\nINCLUDE c.ink\n== hub ==\ntext\n";

  it("folds the block to one line reading the Rust collapsed_text", () => {
    const v = mount(THREE);
    const region = foldLine(v, 0);

    expect(region).not.toBeNull();
    // The fold hides everything after the first INCLUDE line's end down to the
    // last INCLUDE line — i.e. lines 1 and 2 collapse under line 0.
    expect(region?.from).toBe(v.state.doc.line(1).to);
    expect(region?.to).toBe(v.state.doc.line(3).to);

    expect(v.dom.querySelector(".brink-fold-include")).not.toBeNull();
    expect(placeholderText(v)).toBe("INCLUDE … (3 files)");
  });

  it("is accessible: the placeholder carries an aria-label and is clickable", () => {
    const v = mount(THREE);
    foldLine(v, 0);
    const el = v.dom.querySelector<HTMLElement>(".brink-fold-include");
    expect(el?.getAttribute("aria-label")).toBe("folded INCLUDE block: INCLUDE … (3 files)");
    expect(typeof el?.onclick).toBe("function");
  });

  it("unfolds cleanly back to the full block", () => {
    const v = mount(THREE);
    const region = foldLine(v, 0);
    expect(foldedRanges(v.state).size).toBe(1);
    if (region) v.dispatch({ effects: unfoldEffect.of(region) });
    expect(foldedRanges(v.state).size).toBe(0);
    expect(v.dom.querySelector(".brink-fold-include")).toBeNull();
  });

  it("does not fold a single leading INCLUDE", () => {
    const v = mount("INCLUDE only.ink\n== hub ==\ntext\n");
    expect(foldLine(v, 0)).toBeNull();
  });

  it("does not fold when there is no leading INCLUDE block", () => {
    const v = mount("== hub ==\ntext\nmore\n");
    // Line 0 is the knot header (a decl fold), not an include fold.
    const region = foldLine(v, 0);
    expect(region).not.toBeNull();
    expect(v.dom.querySelector(".brink-fold-include")).toBeNull();
  });

  it("leaves the whole-declaration fold placeholder unchanged", () => {
    const v = mount("== hub ==\ntext\n");
    foldLine(v, 0);
    // A decl fold renders its hidden header, not an include label.
    expect(v.dom.querySelector(".brink-fold-include")).toBeNull();
    expect(placeholderText(v)).toBe("== hub ==");
  });

  // Regression (#337): the core also emits a `{...}` sentinel `collapsed_text`
  // on conditional/sequence folds. Presence of `collapsed_text` alone must NOT
  // route a fold through the INCLUDE placeholder — only the leading INCLUDE
  // block (whose `collapsed_text` starts with "INCLUDE") may.
  const COND =
    "INCLUDE a.ink\nINCLUDE b.ink\n== hub ==\n{ cond:\n  - a\n  - else: b\n}\ntail\n";

  it("does not render a folded multi-line conditional as an INCLUDE block", () => {
    const v = mount(COND);
    // Line 3 is the `{ cond:` opener; its fold carries `collapsed_text: "{...}"`.
    const region = foldLine(v, 3);
    expect(region).not.toBeNull();
    // It must fall through to the default placeholder, never the include one.
    expect(v.dom.querySelector(".brink-fold-include")).toBeNull();
    // Default CM fold placeholder: "…" with the generic "folded code" label.
    const el = v.dom.querySelector<HTMLElement>(".cm-foldPlaceholder");
    expect(el).not.toBeNull();
    expect(el?.textContent).toBe("…");
    expect(el?.getAttribute("aria-label")).toBe("folded code");
    // The literal sentinel text must never surface to the user.
    expect(v.dom.textContent ?? "").not.toContain("{...}");
  });

  it("still folds the INCLUDE block correctly when conditionals are present", () => {
    const v = mount(COND);
    foldLine(v, 0);
    expect(v.dom.querySelector(".brink-fold-include")).not.toBeNull();
    expect(placeholderText(v)).toBe("INCLUDE … (2 files)");
  });
});
