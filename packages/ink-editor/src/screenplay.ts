import { type Extension, RangeSetBuilder, EditorState, Annotation } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { elementTypeField, elementClass, ElementType, type LineInfo } from "./element-type.js";

// ── Screenplay sigil geometry (#368) ────────────────────────────────
//
// Character lines are `@Name:<>` (hidden suffix `:<>`, 3 chars); parentheticals
// are `(text)<>` (hidden glue `<>`, 2 chars) — the at-cue preset's shape. These
// are no longer hardcoded constants: every hidden-region / content-region byte
// range is derived from the resolved dialect's match indices, computed ONCE at
// classification time (`element-type.ts`'s `LineInfo.dialect`) and cached
// there. The functions below read that cache — they never re-match a pattern,
// so a custom dialect's geometry (different affix lengths, multiple hidden
// groups, …) works with zero changes here.

export interface SigilLine {
  readonly from: number;
  readonly to: number;
  readonly text: string;
}

/** Leading-whitespace length of a line's text. */
export function leadingWsLen(text: string): number {
  return text.length - text.trimStart().length;
}

/**
 * A line's dialect hidden/content geometry, resolved to absolute document
 * offsets (the cached `LineInfo.dialect` spans are line-relative UTF-16
 * offsets; this adds `line.from`). `null` when the line has no dialect match.
 */
export interface ResolvedSigilGeometry {
  readonly hiddenSpans: readonly (readonly [number, number])[];
  readonly contentSpan: readonly [number, number] | null;
}

/** Resolve a classified line's cached dialect geometry to absolute document
 *  offsets. Returns `null` for lines with no dialect match (plain narrative,
 *  structural lines, chain-only kinds with no geometry of their own). */
export function sigilGeometry(line: SigilLine, info: LineInfo): ResolvedSigilGeometry | null {
  const d = info.dialect;
  if (!d) return null;
  return {
    hiddenSpans: d.hiddenSpans.map(([s, e]) => [line.from + s, line.from + e] as const),
    contentSpan: d.contentSpan ? [line.from + d.contentSpan[0], line.from + d.contentSpan[1]] : null,
  };
}

/**
 * Editable content region of a dialect-classified line (internal —
 * `characterName()`/`contentRegion()` used to be exported; per #368 this is
 * no longer part of the public surface). Falls back to the whole line
 * (minus leading whitespace) when the line has no cached `contentSpan` (the
 * pattern-less-kind contract: content is the whole trimmed line).
 */
function contentRegion(line: SigilLine, info: LineInfo): { start: number; end: number; text: string } {
  const geometry = sigilGeometry(line, info);
  if (geometry?.contentSpan) {
    const [start, end] = geometry.contentSpan;
    return { start, end, text: line.text.slice(start - line.from, end - line.from) };
  }
  const ws = leadingWsLen(line.text);
  return { start: line.from + ws, end: line.to, text: line.text.slice(ws) };
}

/**
 * Editable content region of a `character`-kind line (the dialect's
 * `contentGroup`, e.g. the name in `@Name:<>`) — used by `keybindings.ts`'s
 * name-surgery handlers (guarded by the caller to only run on `Character`
 * lines). All positions are absolute document offsets except `ws` (a
 * length). Package-internal: not re-exported from `index.ts` — hosts
 * needing cast/character data use the future `detectCast` (#366).
 */
export function characterName(line: SigilLine, info: LineInfo): {
  ws: number;
  nameStart: number;
  nameEnd: number;
  name: string;
} {
  const ws = leadingWsLen(line.text);
  const region = contentRegion(line, info);
  return { ws, nameStart: region.start, nameEnd: region.end, name: region.text };
}

class EmptySigilWidget extends WidgetType {
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "brink-hidden-sigil";
    span.textContent = "​"; // zero-width space for cursor anchoring
    return span;
  }
  eq(): boolean { return true; }
}

// ── Line decorations ──────────────────────────────────────────────

function buildLineDecos(view: EditorView): DecorationSet {
  const infos = view.state.field(elementTypeField);
  const builder = new RangeSetBuilder<Decoration>();

  // Section starts: each knot header line, or — when a contiguous /// doc
  // block precedes it — the first doc line. The doc block is part of the
  // knot, so the dividing rule sits above it.
  const sectionStarts = new Set<number>();
  for (let i = 1; i <= view.state.doc.lines; i++) {
    const info = infos[i - 1];
    if (!info || info.type !== ElementType.KnotHeader) continue;
    let first = i;
    for (let j = i - 1; j >= 1; j--) {
      if (view.state.doc.line(j).text.trimStart().startsWith("///")) first = j;
      else break;
    }
    sectionStarts.add(first);
  }

  for (let i = 1; i <= view.state.doc.lines; i++) {
    const line = view.state.doc.line(i);
    const info = infos[i - 1];
    if (!info || info.type === ElementType.Blank) continue;

    const cls = sectionStarts.has(i)
      ? `${elementClass(info.type)} brink-section-start`
      : elementClass(info.type);
    const attrs: Record<string, string> = { class: cls };

    // Option identity (#364): choice lines and their body lines carry which
    // option they belong to, so hosts can style per-branch rails without
    // re-deriving the weave. `data-option-path` is the contract (full lineage
    // through nested weaves, e.g. "0.2.1"); `data-option` is the convenience
    // innermost index.
    if (info.optionPath !== undefined && info.optionPath.length > 0) {
      attrs["data-option-path"] = info.optionPath.join(".");
      attrs["data-option"] = String(info.optionPath[info.optionPath.length - 1]);
    }

    // Dialect attrs (#368): a chained run's carried groups (e.g. `speaker`)
    // ride as `data-*` attributes on the whole run, per the dialect's
    // `chain.carry` contract. Non-carry attrs on a directly-matched line
    // (any named group beyond contentGroup/hidden) surface the same way.
    if (info.dialect) {
      for (const [name, value] of info.dialect.attrs) {
        attrs[`data-${name}`] = value;
      }
    }

    // Weave depth (#414): carried as `data-depth` for choices/gathers at
    // depth > 1 — the indent itself (padding-left, scaled by depth) is a
    // `brinkTheme` rule keyed off the attribute, not an inline style, so
    // headless hosts can restyle or ignore it freely.
    if (
      (info.type === ElementType.Choice || info.type === ElementType.Gather) &&
      info.depth > 1
    ) {
      attrs["data-depth"] = String(info.depth);
    }

    // Standalone diverts (#414): `brink-divert-standalone` class carries the
    // "screenplay transition" look (right-align) via `brinkTheme`, not an
    // inline style.
    if (info.type === ElementType.Divert && info.standalone) {
      attrs.class = `${attrs.class} brink-divert-standalone`;
    }

    builder.add(line.from, line.from, Decoration.line({ attributes: attrs }));

    // NOTE (ruled 2026-08-23, "literal whitespace"): nested choice/gather
    // sigil runs are no longer collapsed into a superscript-depth widget —
    // the file's own sigils render as typed. `data-depth` (above) remains
    // the machine-readable depth for hosts that want a styled look.

    // Dialect hidden-geometry decorations (#368): every hidden span the
    // dialect computed at classification time (the `@`/`:<>` sigils on a
    // character cue, the `<>` glue on a parenthetical, or whatever a custom
    // dialect's affixes are) renders as a hidden zero-width-space widget —
    // no per-kind branching, no re-matching.
    const geometry = sigilGeometry(line, info);
    if (geometry) {
      for (const [start, end] of geometry.hiddenSpans) {
        if (end > start) {
          builder.add(start, end, Decoration.replace({ widget: new EmptySigilWidget() }));
        }
      }
    }
  }

  return builder.finish();
}

const screenplayPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildLineDecos(view);
    }

    update(update: ViewUpdate) {
      // Line decorations span the whole doc and derive only from
      // elementTypeField (which recomputes on docChanged), so a pure
      // viewport scroll needs no rebuild — keep the existing set (#14).
      if (update.docChanged) {
        this.decorations = buildLineDecos(update.view);
      }
    }
  },
  {
    decorations: (v) => v.decorations,
  },
);

// ── Bracket mark decorations ──────────────────────────────────────

function buildBracketDecos(view: EditorView): DecorationSet {
  const infos = view.state.field(elementTypeField);
  const builder = new RangeSetBuilder<Decoration>();

  for (let i = 1; i <= view.state.doc.lines; i++) {
    const line = view.state.doc.line(i);
    const info = infos[i - 1];
    if (!info || info.type !== ElementType.Choice) continue;

    const text = line.text;
    let bracketStart = -1;
    for (let j = 0; j < text.length; j++) {
      if (text[j] === "[") {
        bracketStart = j;
      }
      if (text[j] === "]" && bracketStart >= 0) {
        builder.add(
          line.from + bracketStart,
          line.from + j + 1,
          Decoration.mark({ class: "brink-choice-bracket" }),
        );
        bracketStart = -1;
      }
    }
  }

  return builder.finish();
}

const bracketPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildBracketDecos(view);
    }

    update(update: ViewUpdate) {
      // Same as the line plugin (#14): bracket marks are doc-wide + field-
      // derived, so a viewport-only scroll keeps the existing set.
      if (update.docChanged) {
        this.decorations = buildBracketDecos(update.view);
      }
    }
  },
  {
    decorations: (v) => v.decorations,
  },
);

// ── Atomic ranges for screenplay sigils ───────────────────────────
// Prevents cursor from landing inside hidden dialect-geometry regions.

const atomicMark = Decoration.mark({});

const screenplayAtomicRanges = EditorView.atomicRanges.of((view) => {
  const infos = view.state.field(elementTypeField);
  const builder = new RangeSetBuilder<Decoration>();

  for (let i = 1; i <= view.state.doc.lines; i++) {
    const line = view.state.doc.line(i);
    const info = infos[i - 1];
    if (!info) continue;

    const geometry = sigilGeometry(line, info);
    if (!geometry) continue;
    for (const [start, end] of geometry.hiddenSpans) {
      if (end > start) builder.add(start, end, atomicMark);
    }
  }

  return builder.finish();
});

/** Annotation to bypass the sigil guard — used by our own key handlers. */
export const sigilBypass = Annotation.define<boolean>();

// ── Transaction filter: protect sigil regions from edits ──────────
// Clamps selections away from sigil regions and blocks changes that touch
// any dialect-hidden geometry on a classified line.

const screenplaySigilGuard = EditorState.transactionFilter.of((tr) => {
  if (!tr.docChanged || tr.annotation(sigilBypass)) return tr;

  const infos = tr.startState.field(elementTypeField);
  let dominated = false;

  tr.changes.iterChanges((fromA, toA) => {
    if (dominated) return;
    const line = tr.startState.doc.lineAt(fromA);
    const info = infos[line.number - 1];
    if (!info) return;

    const geometry = sigilGeometry(line, info);
    if (!geometry) return;

    // Block if the change touches ANY hidden span outright (mirrors the old
    // Parenthetical rule: `toA > glueStart` blocks touching the trailing
    // glue).
    for (const [start, end] of geometry.hiddenSpans) {
      if (fromA < end && toA > start) {
        dominated = true;
        return;
      }
    }
    // A content span flanked by a hidden span clamps edits to strictly
    // inside it on that flanked side (mirrors the old Character rule: both
    // `fromA < nameStart` and `toA > nameEnd` block, because BOTH sides are
    // hidden-flanked). A content span with no hidden span on a given side
    // (e.g. Parenthetical's leading paren, which is content, not hidden) is
    // unconstrained on that side — reproduces "only `toA > glueStart` blocks"
    // exactly, since Parenthetical has no hidden span before its content.
    if (geometry.contentSpan) {
      const [start, end] = geometry.contentSpan;
      const hasLeftWall = geometry.hiddenSpans.some(([, hEnd]) => hEnd === start);
      const hasRightWall = geometry.hiddenSpans.some(([hStart]) => hStart === end);
      if (hasLeftWall && fromA < start) dominated = true;
      if (hasRightWall && toA > end) dominated = true;
    }
  });

  return dominated ? [] : tr;
});

/**
 * The screenplay-specific layer: decorations, atomic ranges, and the sigil
 * edit-guard. Does NOT include `elementTypeField` — that field drives
 * structural classification generally (Choice/Gather/Divert/… depth,
 * StatusBar, folding, transitions) independent of any dialect, so it is
 * included unconditionally by `extensions.ts` regardless of the `dialect`
 * option. `dialect: null` (#368 deliverable 5) tears down exactly this
 * bundle — classification keeps running (a line simply never gets a
 * `dialect` kind), but no screenplay-specific rendering/editing behavior
 * remains.
 */
export function screenplayDecorations(): Extension {
  return [screenplayPlugin, bracketPlugin, screenplayAtomicRanges, screenplaySigilGuard];
}
