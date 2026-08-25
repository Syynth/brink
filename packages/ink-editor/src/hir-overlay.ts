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
import { DEFER_LINE_THRESHOLD, deferredRefresh } from "./deferred-refresh.js";
import { isPerfEnabled, perfRecord, perfTime } from "./perf/probe.js";

export interface HirOverlayOptions {
  /** Fetch the current projection from the document's wasm session. */
  getHirProjection: () => HirProjection;
  /** Async warm-up for the projection pull (W2b): runs before the
   *  deferred refresh dispatches so the field's synchronous rebuild (and
   *  the occurrence field riding the same effect) hits the warmed memo.
   *  See `DeferredPrepare`. */
  prepareProjection?: () => Promise<unknown> | undefined;
}

// ── The canonical StateField ────────────────────────────────────────

interface HirOverlayState {
  projection: HirProjection;
  /** Inline marks for non-container spans. */
  marks: DecorationSet;
  /** Per-line rail attributes for lines inside at least one container. */
  lineDecos: DecorationSet;
  /**
   * Container spans by handle, built once per projection (#3067): the rails
   * gutter's `lineMarker` runs once per visible line per rebuild, and
   * constructing this map inside it made scrolling pay
   * O(spans × visible lines) — 19.7 ms per rebuild batch, ~1.5 s per full
   * scroll pass on the perf-fixture large file (desktop-perf baseline).
   */
  spansByHandle: Map<number, HirSpan>;
}

const EMPTY_PROJECTION: HirProjection = { spans: [], lines: [] };

const emptyState: HirOverlayState = {
  projection: EMPTY_PROJECTION,
  marks: Decoration.none,
  lineDecos: Decoration.none,
  spansByHandle: new Map(),
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

  const spansByHandle = new Map<number, HirSpan>();
  for (const s of projection.spans) {
    if (s.handle !== undefined) spansByHandle.set(s.handle, s);
  }

  return {
    projection,
    marks: Decoration.set(marks, true),
    lineDecos: Decoration.set(lineDecos, true),
    spansByHandle,
  };
}

function createOverlayField(options: HirOverlayOptions) {
  const fetchState = (doc: EditorState["doc"]): HirOverlayState | null => {
    try {
      return perfTime("cm.hirOverlay.buildState", () =>
        buildState(options.getHirProjection(), doc),
      );
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
      // #3064 C2 adaptive deferral: in a LARGE document, a doc change maps
      // the existing decorations through the edit (positions stay correct;
      // content refreshes on the debounced `refreshHirOverlayEffect` the
      // `deferredRefresh` plugin dispatches after the burst ends). Small
      // documents rebuild synchronously as before — no staleness window.
      if (tr.docChanged && !refresh && tr.newDoc.lines >= DEFER_LINE_THRESHOLD) {
        return {
          projection: value.projection,
          marks: value.marks.map(tr.changes),
          lineDecos: value.lineDecos.map(tr.changes),
          spansByHandle: value.spansByHandle,
        };
      }
      const fresh = fetchState(tr.newDoc);
      if (fresh) return fresh;
      // R5: transient failure — keep last-good, remapped through the change
      // (a no-op remap for an effect-only transaction).
      return {
        projection: value.projection,
        marks: value.marks.map(tr.changes),
        lineDecos: value.lineDecos.map(tr.changes),
        // Projection is carried unchanged, so its handle map stays valid.
        spansByHandle: value.spansByHandle,
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

/** Display names for the rail kinds. */
const RAIL_KIND_NAMES: Record<string, string> = {
  knot: "Knot",
  stitch: "Stitch",
  choice: "Choice",
  gather: "Gather",
  cond_branch: "Conditional branch",
  seq_branch: "Sequence branch",
};

/** One rail bar's resolved display facts. */
interface RailInfo {
  kind: string;
  depth: number;
  handle: number;
  /** 1-based inclusive line range of the container. */
  startLine: number;
  endLine: number;
  /** The container's own first line of text (name / choice text), trimmed. */
  label: string;
  /** Choice color bucket (0–7, golden-step by handle); absent for other kinds. */
  hue?: number;
}

// ── Rail tooltip (a real floating tooltip, not `title`) ─────────────
//
// One shared element per document; shown on rail hover, positioned beside
// the gutter at the pointer's row. Styled by the host via
// `.brink-rail-tooltip` (studio editor.css).

let railTooltip: HTMLElement | null = null;

function hideRailTooltip(): void {
  railTooltip?.remove();
  railTooltip = null;
}

function showRailTooltip(anchor: HTMLElement, info: RailInfo): void {
  hideRailTooltip();
  const tip = document.createElement("div");
  tip.className = "brink-rail-tooltip";
  const label = tip.appendChild(document.createElement("div"));
  label.className = "brink-rail-tooltip-label";
  const dot = label.appendChild(document.createElement("span"));
  dot.className =
    `brink-rail-tooltip-dot brink-hir-rail-${info.kind}` +
    (info.hue !== undefined ? ` brink-rail-c${info.hue}` : "");
  label.appendChild(document.createTextNode(info.label === "" ? "(empty line)" : info.label));
  const meta = tip.appendChild(document.createElement("div"));
  meta.className = "brink-rail-tooltip-meta";
  meta.textContent =
    info.startLine === info.endLine
      ? `${RAIL_KIND_NAMES[info.kind] ?? info.kind} · line ${info.startLine}`
      : `${RAIL_KIND_NAMES[info.kind] ?? info.kind} · lines ${info.startLine}–${info.endLine}`;
  // Inside the .brink-studio root, or the --bs-* tokens don't resolve and
  // the chrome (background, border, shadow) silently disappears.
  (anchor.closest(".brink-studio") ?? document.body).appendChild(tip);
  const r = anchor.getBoundingClientRect();
  tip.style.setProperty("--brink-popup-left", `${Math.round(r.right + 10)}px`);
  tip.style.setProperty("--brink-popup-top", `${Math.round(r.top)}px`);
  railTooltip = tip;
}

/** The container's display label from its own first line: knots/stitches
 *  show their bare name, choices/gathers their text without the sigils. */
function railLabel(kind: string, raw: string): string {
  let text = raw;
  if (kind === "knot" || kind === "stitch") {
    text = text.replace(/^=+\s*/, "").replace(/\s*=+$/, "");
  } else if (kind === "choice") {
    text = text.replace(/^[*+\s]+/, "");
    // INTERIM heuristic (#3055): skip the `{condition}` guard(s); show the
    // text before and inside the `[]`. To be replaced by a CST-computed
    // label on the wire span — ruled "do this properly".
    while (text.startsWith("{")) {
      let depth = 0;
      let end = -1;
      for (let i = 0; i < text.length; i++) {
        if (text[i] === "{") depth++;
        else if (text[i] === "}" && --depth === 0) {
          end = i;
          break;
        }
      }
      if (end < 0) break;
      text = text.slice(end + 1).trimStart();
    }
    const close = text.indexOf("]");
    if (close >= 0) text = text.slice(0, close + 1);
  } else if (kind === "gather") {
    text = text.replace(/^[-\s]+(?!>)/, "");
  } else if (kind === "cond_branch") {
    // Show the CONDITION: strip the `{` / `-` opener and keep what's
    // before the branch colon (`{ torch < 0:` -> `torch < 0`).
    text = text.replace(/^[{\-\s]+/, "");
    const colon = text.lastIndexOf(":");
    if (colon >= 0) text = text.slice(0, colon).trim();
  }
  return text.slice(0, 60);
}

class RailMarker extends GutterMarker {
  constructor(private readonly stack: readonly RailInfo[]) {
    super();
  }

  override eq(other: RailMarker): boolean {
    return (
      this.stack.length === other.stack.length &&
      this.stack.every((c, i) => {
        const o = other.stack[i];
        return (
          o !== undefined &&
          c.kind === o.kind &&
          c.depth === o.depth &&
          c.hue === o.hue &&
          c.label === o.label &&
          c.startLine === o.startLine &&
          c.endLine === o.endLine
        );
      })
    );
  }

  override toDOM(): Node {
    // Two-element structure (WebKit layout pathology, 2026-08-25): a
    // percent-height (`height: 100%`) inline-flex wrapper inside every
    // gutter element made EVERY forced layout cost ~1 ms per marker in
    // WebKit — ~110 ms per keystroke/refresh on a real project, the
    // dominant slice of the desktop app's typing latency (Chromium never
    // showed it). The fix: an in-flow spacer whose fixed pixel WIDTH
    // sizes the gutter, plus an absolutely-positioned bar layer anchored
    // to the gutter element's full height (`.brink-hir-rails-bars`,
    // `inset: 0 auto 0 0`) — same visuals, no percent-height resolution
    // on the layout hot path (measured 120 ms → 36 ms full-layout).
    const wrap = document.createElement("span");
    wrap.className = "brink-hir-rails";
    // bars are 3px + 2px gap, padding 0 2px: width = 5n + 2 (n ≥ 1).
    wrap.style.width = `${this.stack.length * 5 + 2}px`;
    const bars = wrap.appendChild(document.createElement("span"));
    bars.className = "brink-hir-rails-bars";
    for (const c of this.stack) {
      const bar = bars.appendChild(document.createElement("span"));
      bar.className =
        `brink-hir-rail brink-hir-rail-${c.kind}` +
        (c.hue !== undefined ? ` brink-rail-c${c.hue}` : "");
      bar.setAttribute("data-depth", String(c.depth));
      bar.addEventListener("pointerenter", () => showRailTooltip(bar, c));
      bar.addEventListener("pointerleave", hideRailTooltip);
    }
    return wrap;
  }

  override destroy(dom: Node): void {
    // The shared tooltip must not outlive the marker that opened it.
    hideRailTooltip();
    super.destroy(dom);
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

  // The rails gutter's lineMarker runs once per visible line per rebuild —
  // recording each call as its own span would flood the probe's ring during
  // a scroll, so calls accumulate here and flush as ONE
  // `cm.hirRails.lineMarkers` span (meta: calls in the batch) per microtask,
  // i.e. per synchronous gutter rebuild.
  let railsAcc: { startMs: number; totalMs: number; count: number } | null = null;
  const accumulateRailsTime = (startMs: number, durMs: number): void => {
    if (railsAcc === null) {
      railsAcc = { startMs, totalMs: 0, count: 0 };
      queueMicrotask(() => {
        if (railsAcc !== null) {
          perfRecord("cm.hirRails.lineMarkers", railsAcc.startMs, railsAcc.totalMs, railsAcc.count);
        }
        railsAcc = null;
      });
    }
    railsAcc.totalMs += durMs;
    railsAcc.count++;
  };

  const buildLineMarker = (view: EditorView, line: { from: number }): RailMarker | null => {
        // #3067: the span-by-handle map is prebuilt on the overlay state
        // (once per projection), not per visible line — building it here
        // made scrolling O(spans × visible lines).
        const { projection, spansByHandle: byHandle } = view.state.field(field);
        const doc = view.state.doc;
        const lineNo = doc.lineAt(line.from).number - 1;
        const stack = projection.lines[lineNo];
        if (!stack || stack.length === 0) return null;
        const infos: RailInfo[] = stack.map((c) => {
          const span = byHandle.get(c.handle);
          const startLine = (span?.start_line ?? lineNo) + 1;
          // Tooltip range = the TIGHT end (actual content), not the
          // structural end that runs to the next sibling (#3054).
          const endLine = (span?.content_end_line ?? span?.end_line ?? lineNo) + 1;
          let raw =
            startLine >= 1 && startLine <= doc.lines ? doc.line(startLine).text.trim() : "";
          if (c.kind === "cond_branch" && !/:\s*$/.test(raw) && !raw.includes(":")) {
            // INTERIM heuristic (#3055): the branch span covers the BODY;
            // its condition sits on the nearest preceding `{ cond:` /
            // `- else:` opener line. CST-computed labels replace this.
            for (let l = startLine - 1; l >= Math.max(1, startLine - 6); l--) {
              const t = doc.line(l).text.trim();
              if (/:\s*$/.test(t)) {
                raw = t;
                break;
              }
              if (t.endsWith("}") || t === "") break;
            }
          }
          const info: RailInfo = {
            kind: c.kind,
            depth: c.depth,
            handle: c.handle,
            startLine,
            endLine,
            label: railLabel(c.kind, raw),
          };
          // Distinct sibling-choice colors: a golden-step permutation over
          // eight theme buckets, keyed by the stable container handle so a
          // choice keeps its color across its whole body.
          if (c.kind === "choice") info.hue = (c.handle * 5) % 8;
          return info;
        });
        return new RailMarker(infos);
  };

  return [
    field,
    // #3064 C2: after a typing burst in a large document, rebuild the
    // deferred overlay content once the doc goes quiet.
    deferredRefresh(
      refreshHirOverlayEffect,
      120,
      options.prepareProjection ? () => options.prepareProjection?.() : undefined,
    ),
    EditorView.decorations.from(field, (v) => v.marks),
    EditorView.decorations.from(field, (v) => v.lineDecos),
    // #3064 micro: occurrence highlights as a field with adaptive
    // deferral — in a LARGE document a doc change maps the existing
    // highlights through the edit and the deferred overlay refresh
    // rebuilds them ~120 ms after the burst (the cadence mainstream
    // editors debounce occurrence highlights to anyway). Pure selection
    // moves (clicks, arrow keys) rebuild immediately, so navigation
    // feels instant; small documents rebuild on every transaction as
    // before.
    StateField.define<DecorationSet>({
      create(state) {
        return perfTime("cm.hirOverlay.occurrences", () => buildOccurrences(state, field));
      },
      update(value, tr) {
        const refresh = tr.effects.some((e) => e.is(refreshHirOverlayEffect));
        if (tr.docChanged && !refresh && tr.newDoc.lines >= DEFER_LINE_THRESHOLD) {
          return value.map(tr.changes);
        }
        if (!tr.docChanged && !refresh && tr.selection === undefined) return value;
        return perfTime("cm.hirOverlay.occurrences", () => buildOccurrences(tr.state, field));
      },
      provide: (f) => EditorView.decorations.from(f),
    }),
    gutter({
      class: "brink-hir-rail-gutter",
      lineMarker(view, line) {
        if (!isPerfEnabled()) return buildLineMarker(view, line);
        const t0 = performance.now();
        try {
          return buildLineMarker(view, line);
        } finally {
          accumulateRailsTime(t0, performance.now() - t0);
        }
      },
      lineMarkerChange: (update) => update.docChanged || update.startState.field(field) !== update.state.field(field),
    }),
  ];
}
