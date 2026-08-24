/**
 * HIR structural overlay (#454 phases 3–5).
 *
 * A CodeMirror layer over the HIR **projection** (`getHirSpansDoc`): the
 * canonical structural model of the document — nested semantic spans plus a
 * per-line container stack — held in a queryable StateField, with three
 * derived surfaces:
 *
 * 1. **Inline marks** (phase 3): every non-container span renders as a
 *    `Decoration.mark` carrying the class taxonomy (`brink-hir-<kind>`) and
 *    `data-*` attributes (`data-hir-kind`, `data-def-id`, `data-target-id`,
 *    `data-hir-depth`), so the DOM is span-addressable for hosts, tests, and
 *    CSS. A reference span with no resolved target additionally carries
 *    `brink-hir-unresolved` / `data-unresolved`.
 * 2. **Rails** (phase 4): per-line `Decoration.line` attributes plus a gutter
 *    of concentric bars — one per container covering the line, outermost →
 *    innermost — each bar class-addressed (`brink-hir-rail`,
 *    `brink-hir-rail-<kind>`, `data-depth`). No inline styles (headless
 *    taxonomy, see editor-consumer-guide).
 * 3. **Identity queries + occurrences** (phase 5): `hirSpanAt` /
 *    `hirIdentityAt` read the StateField; the occurrences layer highlights
 *    every span sharing the identity under the cursor (`brink-hir-occurrence`,
 *    with `-def` on the declaration). Hover cards and go-to-definition already
 *    exist in the IDE layer (`hover_doc` / `goto_definition_doc`) — the
 *    overlay adds the identity-keyed *occurrence* surface they lack.
 *
 * Robustness (spec R5): the projection is re-fetched on every doc change (the
 * wasm session was already updated by the elementTypeField StateField, which
 * runs first); if the fetch throws, the previous decorations are **remapped
 * through the change** instead of dropped, so the overlay never flickers on a
 * transient failure.
 */

import {
  StateEffect,
  StateField,
  type EditorState,
  type Extension,
  type Transaction,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  GutterMarker,
  gutter,
} from "@codemirror/view";
import type { HirProjection, HirSpan } from "@brink/wasm-types";

export interface HirOverlayOptions {
  /** Fetch the current projection from the document's wasm session. */
  getHirProjection: () => HirProjection;
}

// ── The canonical StateField ────────────────────────────────────────

interface HirOverlayState {
  projection: HirProjection;
  /** Inline marks for non-container spans. */
  marks: DecorationSet;
  /** Per-line rail attributes for lines inside at least one container. */
  lineDecos: DecorationSet;
}

const EMPTY_PROJECTION: HirProjection = { spans: [], lines: [] };

const emptyState: HirOverlayState = {
  projection: EMPTY_PROJECTION,
  marks: Decoration.none,
  lineDecos: Decoration.none,
};

/**
 * Force the overlay to re-read `getHirProjection` without a doc change
 * (#494). The StateField seeds at view creation — before the first async
 * compile/analysis completes — and otherwise only recomputes on doc-changing
 * transactions, so a passive load would keep the empty seed until the first
 * edit. `DocumentSessions` dispatches this to every mounted view when a
 * compile result is delivered, AND to a view that mounts after a compile was
 * already delivered (#518 — the delivery-time loop only reaches views that
 * exist; a later mount self-serves the refresh, covering both mount orders
 * and remounts that reuse a cached EditorState, where `create()` never
 * re-runs). Hosts with custom wiring can dispatch it (or call
 * `refreshHirOverlay(view)`) from their own compile-complete signal.
 * Mirrors `reclassifyEffect` / `refreshGutterMarkersEffect`.
 *
 * NOTE for hosts dispatching this themselves: the effect is matched by
 * object identity (`e.is(refreshHirOverlayEffect)`), so it must come from
 * the SAME module instance of `@brink-lang/editor` that built the view's
 * extensions. A bundler that duplicates the package (e.g. an app importing
 * it directly while also consuming a library that bundled its own copy)
 * produces an effect the field silently ignores.
 */
export const refreshHirOverlayEffect = StateEffect.define<void>();

/** Convenience wrapper: dispatch `refreshHirOverlayEffect` on `view`. */
export function refreshHirOverlay(view: EditorView): void {
  view.dispatch({ effects: refreshHirOverlayEffect.of(undefined) });
}

/** Doc position of a (0-based line, UTF-16 col), or null when out of range. */
function posOf(
  doc: EditorState["doc"],
  line: number,
  char: number,
): number | null {
  const lineNum = line + 1;
  if (lineNum < 1 || lineNum > doc.lines) return null;
  const l = doc.line(lineNum);
  const pos = l.from + char;
  return pos > l.to ? null : pos;
}

function buildState(projection: HirProjection, doc: EditorState["doc"]): HirOverlayState {
  const marks: ReturnType<Decoration["range"]>[] = [];
  for (const s of projection.spans) {
    if (s.container) continue;
    const from = posOf(doc, s.start_line, s.start_char);
    const to = posOf(doc, s.end_line, s.end_char);
    if (from === null || to === null || to <= from) continue;

    const unresolved =
      s.target_id === undefined &&
      (s.kind === "divert" || s.kind === "var_ref" || s.kind === "call");
    const attributes: Record<string, string> = { "data-hir-kind": s.kind };
    if (s.def_id !== undefined) attributes["data-def-id"] = s.def_id;
    if (s.target_id !== undefined) attributes["data-target-id"] = s.target_id;
    attributes["data-hir-depth"] = String(s.depth);
    if (unresolved) attributes["data-unresolved"] = "";

    marks.push(
      Decoration.mark({
        class: `brink-hir-${s.kind}${unresolved ? " brink-hir-unresolved" : ""}`,
        attributes,
      }).range(from, to),
    );
  }

  const lineDecos: ReturnType<Decoration["range"]>[] = [];
  for (let i = 0; i < projection.lines.length; i++) {
    const stack = projection.lines[i];
    if (!stack || stack.length === 0) continue;
    const lineNum = i + 1;
    if (lineNum > doc.lines) break;
    const line = doc.line(lineNum);
    lineDecos.push(
      Decoration.line({
        attributes: {
          "data-hir-rails": stack.map((c) => c.kind).join(" "),
          "data-hir-rail-count": String(stack.length),
        },
      }).range(line.from),
    );
  }

  return {
    projection,
    marks: Decoration.set(marks, true),
    lineDecos: Decoration.set(lineDecos, true),
  };
}

function createOverlayField(options: HirOverlayOptions) {
  const fetchState = (doc: EditorState["doc"]): HirOverlayState | null => {
    try {
      return buildState(options.getHirProjection(), doc);
    } catch {
      return null;
    }
  };

  return StateField.define<HirOverlayState>({
    create(state) {
      return fetchState(state.doc) ?? emptyState;
    },
    update(value, tr: Transaction) {
      const refresh = tr.effects.some((e) => e.is(refreshHirOverlayEffect));
      if (!tr.docChanged && !refresh) return value;
      const fresh = fetchState(tr.newDoc);
      if (fresh) return fresh;
      // R5: transient failure — keep last-good, remapped through the change
      // (a no-op remap for an effect-only transaction).
      return {
        projection: value.projection,
        marks: value.marks.map(tr.changes),
        lineDecos: value.lineDecos.map(tr.changes),
      };
    },
  });
}

// ── Identity queries (phase 5 substrate) ────────────────────────────

/** The non-container span(s) covering `pos`, innermost last. */
export function hirSpansAt(
  projection: HirProjection,
  doc: EditorState["doc"],
  pos: number,
): HirSpan[] {
  const hits: HirSpan[] = [];
  for (const s of projection.spans) {
    if (s.container) continue;
    const from = posOf(doc, s.start_line, s.start_char);
    const to = posOf(doc, s.end_line, s.end_char);
    if (from === null || to === null) continue;
    if (pos >= from && pos <= to) hits.push(s);
  }
  return hits;
}

/**
 * The symbol identity at `pos`: a declaration's `def_id` or a reference's
 * resolved `target_id`, whichever an identity-bearing span under the cursor
 * carries. `null` when the cursor is not on an identity-bearing span.
 */
export function hirIdentityAt(
  projection: HirProjection,
  doc: EditorState["doc"],
  pos: number,
): string | null {
  const spans = hirSpansAt(projection, doc, pos);
  for (let i = spans.length - 1; i >= 0; i--) {
    const s = spans[i];
    const id = s?.def_id ?? s?.target_id;
    if (id !== undefined) return id;
  }
  return null;
}

// ── Rails gutter (phase 4) ──────────────────────────────────────────

/** Human labels for the rail kinds (hover tooltips). */
const RAIL_LABELS: Record<string, string> = {
  knot: "Knot body",
  stitch: "Stitch body",
  choice: "Choice branch",
  gather: "Gather continuation",
  cond_branch: "Conditional branch",
  seq_branch: "Sequence branch",
};

class RailMarker extends GutterMarker {
  constructor(private readonly stack: readonly { kind: string; depth: number }[]) {
    super();
  }

  override eq(other: RailMarker): boolean {
    return (
      this.stack.length === other.stack.length &&
      this.stack.every(
        (c, i) => c.kind === other.stack[i]?.kind && c.depth === other.stack[i]?.depth,
      )
    );
  }

  override toDOM(): Node {
    const wrap = document.createElement("span");
    wrap.className = "brink-hir-rails";
    for (const c of this.stack) {
      const bar = wrap.appendChild(document.createElement("span"));
      bar.className = `brink-hir-rail brink-hir-rail-${c.kind}`;
      bar.setAttribute("data-depth", String(c.depth));
      // Hover explanation — the rails are otherwise unlabeled marks.
      bar.title = `${RAIL_LABELS[c.kind] ?? c.kind} — structure rail, nesting depth ${c.depth}`;
    }
    return wrap;
  }
}

// ── Occurrences (phase 5) ───────────────────────────────────────────

function buildOccurrences(
  state: EditorState,
  field: StateField<HirOverlayState>,
): DecorationSet {
  const { projection } = state.field(field);
  const identity = hirIdentityAt(projection, state.doc, state.selection.main.head);
  if (identity === null) return Decoration.none;

  const decos: ReturnType<Decoration["range"]>[] = [];
  for (const s of projection.spans) {
    const isDef = s.def_id === identity;
    if (!isDef && s.target_id !== identity) continue;
    const from = posOf(state.doc, s.start_line, s.start_char);
    const to = posOf(state.doc, s.end_line, s.end_char);
    if (from === null || to === null || to <= from) continue;
    decos.push(
      Decoration.mark({
        class: isDef
          ? "brink-hir-occurrence brink-hir-occurrence-def"
          : "brink-hir-occurrence",
      }).range(from, to),
    );
  }
  return Decoration.set(decos, true);
}

// ── The extension bundle ────────────────────────────────────────────

/**
 * The HIR overlay: canonical projection StateField + inline marks + per-line
 * rail attributes + rails gutter + identity occurrences. Composes with (does
 * not replace) the `tok-*` semantic-token highlight layer.
 */
export function hirOverlayExtension(options: HirOverlayOptions): Extension {
  const field = createOverlayField(options);

  return [
    field,
    EditorView.decorations.from(field, (v) => v.marks),
    EditorView.decorations.from(field, (v) => v.lineDecos),
    EditorView.decorations.compute([field, "selection"], (state) =>
      buildOccurrences(state, field),
    ),
    gutter({
      class: "brink-hir-rail-gutter",
      lineMarker(view, line) {
        const { projection } = view.state.field(field);
        const lineNo = view.state.doc.lineAt(line.from).number - 1;
        const stack = projection.lines[lineNo];
        if (!stack || stack.length === 0) return null;
        return new RailMarker(stack);
      },
      lineMarkerChange: (update) => update.docChanged || update.startState.field(field) !== update.state.field(field),
    }),
  ];
}
