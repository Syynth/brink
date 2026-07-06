import {
  Compartment,
  Facet,
  Prec,
  StateField,
  type Extension,
  type EditorState,
  type Transaction,
} from "@codemirror/state";
import { foldService, codeFolding, foldEffect, unfoldEffect } from "@codemirror/language";
import type { EditorView, Command } from "@codemirror/view";
import type { FoldKind, FoldRange } from "@brink/wasm-types";
import { elementTypeField, type LineInfo } from "./element-type.js";

export type { FoldKind } from "@brink/wasm-types";

export interface FoldingOptions {
  getFoldingRanges: (source: string) => FoldRange[];
}

// ── Fold ranges as a StateField (#365) ──────────────────────────────
//
// `foldRangesFacet` carries the host's `getFoldingRanges` callback (provided
// once, at `foldingExtension(options)` construction); `foldRangesField`
// recomputes the `FoldRange[]` from it whenever the doc changes, and is the
// SINGLE source both the fold service and the module-level
// `foldAllOfKind`/`unfoldAllOfKind` commands read — no per-extension-
// instance closure state, so the bulk commands work against whatever view
// they're dispatched on.
const foldRangesFacet = Facet.define<FoldingOptions["getFoldingRanges"], FoldingOptions["getFoldingRanges"]>({
  combine: (values) => values[0] ?? (() => []),
});

function computeFoldRanges(state: EditorState): FoldRange[] {
  const getFoldingRanges = state.facet(foldRangesFacet);
  try {
    return getFoldingRanges(state.doc.toString());
  } catch {
    return [];
  }
}

const foldRangesField = StateField.define<FoldRange[]>({
  create(state) {
    return computeFoldRanges(state);
  },
  update(value, tr: Transaction) {
    if (!tr.docChanged) return value;
    return computeFoldRanges(tr.state);
  },
});

// ── Active fold kinds (#365 — Celeris §5.5) ─────────────────────────
//
// Which `FoldKind`s the fold service will actually fold. Live-reconfigurable
// per view, mirroring `dialectFacet`'s own compartment/facet pattern
// (`element-type.ts`): a narrative-lens view wants structural+machinery
// active (collapse the logic, read the prose); a logic-focused view wants
// structural+narrative active; "hybrid" wants structural only (nothing
// auto-collapses). Mode auto-collapse is host-invoked — the host runs
// `foldAllOfKind(kind)` on mode entry; this facet only controls whether a
// kind is foldABLE at all, it never forces a collapse itself.
const DEFAULT_ACTIVE_KINDS: ReadonlySet<FoldKind> = new Set([
  "structural",
  "machinery",
  "narrative",
]);

export const activeFoldKindsFacet = Facet.define<ReadonlySet<FoldKind>, ReadonlySet<FoldKind>>({
  combine: (values) => (values.length > 0 ? values[0] : DEFAULT_ACTIVE_KINDS),
});

/** Dedicated compartment for `activeFoldKindsFacet`, so
 *  `setActiveFoldKinds(view, kinds)` can reconfigure it independent of
 *  whatever compartment hosts the rest of the folding extension. */
export const activeFoldKindsCompartment = new Compartment();

/** Live-reconfigure a mounted view's active fold-kinds set (#365). Kinds
 *  removed from the set are simply no longer foldABLE at that line (existing
 *  folds of a removed kind are left folded — this only gates future
 *  fold-service queries, it never force-unfolds). */
export function setActiveFoldKinds(view: EditorView, kinds: ReadonlySet<FoldKind>): void {
  view.dispatch({
    effects: activeFoldKindsCompartment.reconfigure(activeFoldKindsFacet.of(kinds)),
  });
}

/** The CM6 fold `{from, to}` a `FoldRange` maps to, plus the range it came
 *  from, so the placeholder can read the Rust-supplied `collapsed_text`. */
interface ResolvedFold {
  from: number;
  to: number;
  range: FoldRange;
}

export function foldingExtension(options: FoldingOptions): Extension {
  const rangesFor = (state: EditorState): FoldRange[] => state.field(foldRangesField);

  const service = foldService.of((state, lineStart, _lineEnd) => {
    const line = state.doc.lineAt(lineStart);
    const lineNum = line.number - 1; // 0-indexed
    const active = state.facet(activeFoldKindsFacet);

    for (const range of rangesFor(state)) {
      if (range.start_line === lineNum && active.has(range.kind)) {
        const resolved = resolveFold(state, range);
        if (resolved) return { from: resolved.from, to: resolved.to };
      }
    }

    return null;
  });

  return [
    foldRangesFacet.of(options.getFoldingRanges),
    foldRangesField,
    activeFoldKindsCompartment.of(activeFoldKindsFacet.of(DEFAULT_ACTIVE_KINDS)),
    service,
    Prec.high(codeFolding(placeholderConfig(rangesFor))),
  ];
}

/** Fold every current [`FoldRange`] of `kind` (#365). A `Command` for hosts
 *  to bind to a keymap or invoke on view-mode entry — auto-collapse is
 *  always HOST-invoked, never forced by the extension itself. Returns
 *  `false` (a no-op command, per CM6 `Command` convention) when there was
 *  nothing to fold. */
export function foldAllOfKind(kind: FoldKind): Command {
  return (view: EditorView): boolean => {
    // A bulk fold must respect the same active-kinds gate the fold service
    // itself enforces — deactivating a kind means it's not foldable at all,
    // bulk command included.
    if (!view.state.facet(activeFoldKindsFacet).has(kind)) return false;
    const effects = collectFoldEffects(view.state, kind, foldEffect);
    if (effects.length === 0) return false;
    view.dispatch({ effects });
    return true;
  };
}

/** Unfold every currently-folded range of `kind` (#365). Per-region unfold by
 *  the user (clicking a placeholder, standard CM6 gutter toggle) is never
 *  affected by this — it's a bulk convenience the host may call, e.g. from a
 *  "show all machinery" toggle. */
export function unfoldAllOfKind(kind: FoldKind): Command {
  return (view: EditorView): boolean => {
    const effects = collectFoldEffects(view.state, kind, unfoldEffect);
    if (effects.length === 0) return false;
    view.dispatch({ effects });
    return true;
  };
}

/** Shared walk for `foldAllOfKind`/`unfoldAllOfKind`: every `FoldRange` of
 *  `kind` present in `foldRangesField`, resolved to its CM6 `{from, to}` and
 *  wrapped in the given effect type. */
function collectFoldEffects(
  state: EditorState,
  kind: FoldKind,
  effectType: typeof foldEffect | typeof unfoldEffect,
) {
  const effects = [];
  for (const range of state.field(foldRangesField, false) ?? []) {
    if (range.kind !== kind) continue;
    const resolved = resolveFold(state, range);
    if (resolved) effects.push(effectType.of({ from: resolved.from, to: resolved.to }));
  }
  return effects;
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
  | { kind: "decl"; header: string | null; declKind: DeclKind | null } // whole-declaration header (or none).
  | { kind: "machinery-pill"; summary: PillSummary } // #365 machinery fold placeholder.
  | { kind: "narrative-pill"; summary: PillSummary }; // #365 narrative fold placeholder.

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
 *  verbatim. Otherwise it dispatches on the matching `FoldRange`'s `kind`
 *  (#365): `machinery`/`narrative` get a JetBrains-style summary pill;
 *  everything else (structural) falls back to the whole-declaration header. */
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
      let matched: FoldRange | null = null;
      for (const fr of rangesFor(state)) {
        const resolved = resolveFold(state, fr);
        if (resolved && resolved.from === range.from && resolved.to === range.to) {
          if (isIncludeBlockFold(fr)) {
            // Non-null: isIncludeBlockFold guarantees a string collapsed_text.
            return { kind: "collapsed", text: fr.collapsed_text as string };
          }
          matched = fr;
          break;
        }
      }

      if (matched?.kind === "machinery") {
        return { kind: "machinery-pill", summary: buildMachinerySummary(state, matched) };
      }
      if (matched?.kind === "narrative") {
        return { kind: "narrative-pill", summary: buildNarrativeSummary(state, matched) };
      }
      const declKind = matched ? declKindFor(state, range) : null;
      return { kind: "decl", header: prepareDeclPlaceholder(state, range), declKind };
    },
    placeholderDOM(
      _view: unknown,
      onclick: (event: Event) => void,
      prepared: FoldPlaceholder,
    ): HTMLElement {
      if (prepared.kind === "collapsed") {
        return includePlaceholderDOM(onclick, prepared.text);
      }
      if (prepared.kind === "machinery-pill") {
        return pillDOM(onclick, "machinery", prepared.summary);
      }
      if (prepared.kind === "narrative-pill") {
        return pillDOM(onclick, "narrative", prepared.summary);
      }
      return declPlaceholderDOM(onclick, prepared.header, prepared.declKind);
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

// ── Decl pill: data-decl-kind + icon slot (#365 deliverable) ────────

export type DeclKind = "knot" | "stitch" | "function";

/** Classify a declaration fold's header line as knot/stitch/function, from
 *  the same header text `prepareDeclPlaceholder` already slices — a knot
 *  header is `===` (2+ `=` on both sides), a stitch header is single `=`,
 *  and a knot header containing the `function` keyword is a function. */
function declKindFor(state: EditorState, range: { from: number; to: number }): DeclKind | null {
  const header = prepareDeclPlaceholder(state, range);
  if (!header) return null;
  const trimmed = header.trim();
  const isKnot = /^={2,}/.test(trimmed);
  if (isKnot) {
    return /^={2,}\s*function\b/.test(trimmed) ? "function" : "knot";
  }
  if (/^=[^=]/.test(trimmed)) return "stitch";
  return null;
}

function declIconGlyph(kind: DeclKind | null): string {
  switch (kind) {
    case "function":
      return "ƒ";
    case "stitch":
      return "§";
    case "knot":
      return "◆";
    default:
      return "";
  }
}

function declPlaceholderDOM(
  onclick: (event: Event) => void,
  prepared: string | null,
  declKind: DeclKind | null,
): HTMLElement {
  const el = document.createElement("span");
  if (prepared) {
    el.className = "brink-fold-decl";
    if (declKind) el.setAttribute("data-decl-kind", declKind);
    const icon = document.createElement("span");
    icon.className = "brink-fold-decl-icon";
    icon.textContent = declIconGlyph(declKind);
    el.appendChild(icon);
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

// ── Machinery/narrative summary pills (#365) ────────────────────────
//
// JetBrains principle: show the pertinent content, not a count. Pill DOM is
// `brink-fold-pill` + a kind class (`brink-fold-pill-machinery` /
// `brink-fold-pill-narrative`) + spans for icon slot / summary / count —
// class-addressable, zero inline styles (the host styles everything).

interface PillSummary {
  items: string[];
  moreCount: number;
  lineCount: number;
  /** Narrative-only: the cast of speakers in the run, via the dialect's
   *  carried `speaker` attr — never a re-hardcoded `characterName()`. */
  cast: string[];
  /** Narrative-only: the first line's snippet (trimmed content). */
  firstLine: string | null;
}

const MAX_PILL_ITEMS = 2;

/** Effects summary for a machinery fold: salient external calls,
 *  assignments, and divert targets from each line's trimmed text — capped
 *  at `MAX_PILL_ITEMS`, with a "+N more" remainder.
 *
 *  Walks the fold's full line range (`start_line`..`end_line`), NOT the
 *  resolved CM6 fold's hidden `{from, to}` — a fold hides from the END of
 *  its anchor line onward (the anchor line itself stays visible in the
 *  editor), but the anchor line is still part of the machinery RUN the
 *  pill summarizes, so it must contribute to the summary too. Mirrors
 *  `prepareDeclPlaceholder`'s existing text-slicing approach — this is a UI
 *  summary, not content/geometry classification. */
function buildMachinerySummary(state: EditorState, range: FoldRange): PillSummary {
  const lines: string[] = [];
  for (let n = range.start_line + 1; n <= range.end_line + 1 && n <= state.doc.lines; n++) {
    const text = state.doc.line(n).text;
    if (text.trim() !== "") lines.push(text);
  }
  const items: string[] = [];
  for (const line of lines) {
    const item = machineryLineSummary(line.trim());
    if (item) items.push(item);
  }
  return {
    items: items.slice(0, MAX_PILL_ITEMS),
    moreCount: Math.max(0, items.length - MAX_PILL_ITEMS),
    lineCount: lines.length,
    cast: [],
    firstLine: null,
  };
}

function machineryLineSummary(trimmed: string): string | null {
  if (trimmed.startsWith("->")) {
    const target = trimmed.replace(/^->\s*/, "").replace(/\s*->\s*$/, "");
    return `→ ${target}`;
  }
  if (trimmed.startsWith("<-")) {
    return `← ${trimmed.replace(/^<-\s*/, "")}`;
  }
  if (trimmed.startsWith("~")) {
    const body = trimmed.replace(/^~\s*/, "");
    // `~ change_party_member(2, false)` (call) vs `~ temp x = 1` (assignment)
    const callMatch = /^([A-Za-z_][\w.]*\()/.exec(body);
    if (callMatch) return `${callMatch[1]}…)`;
    return body;
  }
  return null;
}

/** Scene summary for a narrative fold: first-line snippet + cast (via the
 *  dialect's carried `speaker` attr, from `elementTypeField`'s cached
 *  `LineInfo.dialect` — never a re-hardcoded `characterName()`) + line
 *  count.
 *
 *  Walks the fold's full line range (`start_line`..`end_line`), NOT the
 *  resolved CM6 fold's hidden `{from, to}` — see `buildMachinerySummary` for
 *  why the anchor line must be included. */
function buildNarrativeSummary(state: EditorState, range: FoldRange): PillSummary {
  const infos = state.field(elementTypeField, false);

  const cast = new Set<string>();
  let firstLine: string | null = null;
  let lineCount = 0;

  for (let n = range.start_line + 1; n <= range.end_line + 1 && n <= state.doc.lines; n++) {
    const line = state.doc.line(n);
    const trimmed = line.text.trim();
    if (trimmed === "") continue;
    lineCount++;
    const info: LineInfo | undefined = infos?.[n - 1];
    if (info?.dialect) {
      const speaker = info.dialect.attrs.find(([k]) => k === "speaker")?.[1];
      if (speaker) cast.add(speaker);
    }
    if (firstLine === null) firstLine = trimmed;
  }

  return { items: [], moreCount: 0, lineCount, cast: [...cast], firstLine };
}

function pillDOM(
  onclick: (event: Event) => void,
  kind: "machinery" | "narrative",
  summary: PillSummary,
): HTMLElement {
  const el = document.createElement("span");
  el.className = `brink-fold-pill brink-fold-pill-${kind}`;

  const icon = document.createElement("span");
  icon.className = "brink-fold-pill-icon";
  icon.textContent = kind === "machinery" ? "⚙" : "❞";
  el.appendChild(icon);

  const label = document.createElement("span");
  label.className = "brink-fold-pill-summary";
  if (kind === "machinery") {
    const parts = [...summary.items];
    if (summary.moreCount > 0) parts.push(`+${summary.moreCount} more`);
    label.textContent = parts.join(" · ");
  } else {
    const snippet = summary.firstLine ?? "";
    label.textContent = summary.cast.length > 0 ? `${snippet} — ${summary.cast.join(", ")}` : snippet;
  }
  el.appendChild(label);

  const count = document.createElement("span");
  count.className = "brink-fold-pill-count";
  count.textContent = `${summary.lineCount} lines`;
  el.appendChild(count);

  el.setAttribute(
    "aria-label",
    kind === "machinery" ? `folded machinery, ${summary.lineCount} lines` : `folded narrative, ${summary.lineCount} lines`,
  );
  el.onclick = onclick;
  return el;
}
