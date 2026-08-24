/**
 * SearchCardBuffer — one result card's editable buffer
 * (docs/search-results-cards-spec.md, PR C).
 *
 * Each visible card in the Search panel mounts one of these: a minimal CM6
 * editor over the card's slice of the source file (the match line plus its
 * context window). Deliberately *not* the full editor extension set — line
 * numbers (offset to the slice's position in the file), the brink theme,
 * semantic-token highlight decorations sliced from the host's per-file
 * token cache, and a mark on the match span. Off-screen and collapsed
 * cards render static HTML through {@link cardLineSegments} instead.
 *
 * Edits write through to the source as ONE slice replacement (the whole
 * card window), committed after an idle pause or on blur — the
 * {@link SearchResultsBuffer} commit contract. The host guards the commit
 * (live slice must still equal the text the card was built from) and owns
 * what happens next: apply → invalidate → compile → snapshot remap →
 * {@link SearchCardBuffer.setCard} feeds the reconciled slice back in.
 *
 * Teardown contract: `destroy()` flushes any pending commit, destroys the
 * EditorView and clears the host — leaks are bugs.
 */

import {
  Annotation,
  Compartment,
  EditorSelection,
  EditorState,
  StateEffect,
  StateField,
  RangeSetBuilder,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, lineNumbers } from "@codemirror/view";
import type { SemanticToken } from "@brink/wasm-types";
import { brinkTheme } from "./theme.js";
import type { ReplacementEdit } from "./project-search.js";
import { DEFAULT_COMMIT_DELAY_MS } from "./search-results-buffer.js";

// ── Model ───────────────────────────────────────────────────────────

/** What one card displays — computed by the host from the snapshot. */
export interface SearchCardModel {
  path: string;
  /** Slice span in the file source (UTF-16) the card doc mirrors. */
  from: number;
  to: number;
  /** 1-based file line number of the slice's first line (gutter offset). */
  firstLine: number;
  /** The slice text (whole lines, no trailing newline). */
  text: string;
  /** Match span within the slice; null when it collapsed to nothing. */
  hit: { from: number; to: number } | null;
}

/** Per-file highlight data from the host's token cache. Token coordinates
 *  are file-absolute (0-based `line`, UTF-16 `start_char`). */
export interface SearchCardHighlight {
  tokens: readonly SemanticToken[];
  typeNames: readonly string[];
}

export interface SearchCardBufferOptions {
  /** Commit the edited slice as one source edit (idle pause / blur). */
  onCommit(path: string, edit: ReplacementEdit): void;
  /** Idle delay before a pending edit commits (0 = synchronous, tests). */
  commitDelayMs?: number;
}

// ── Decorations ─────────────────────────────────────────────────────

const tokenDecoCache = new Map<string, Decoration>();

function tokenDeco(typeName: string): Decoration {
  let deco = tokenDecoCache.get(typeName);
  if (!deco) {
    deco = Decoration.mark({ class: "tok-" + typeName });
    tokenDecoCache.set(typeName, deco);
  }
  return deco;
}

const hitDeco = Decoration.mark({ class: "brink-search-hit" });

/** Card-relative decoration ranges for a model + its file's highlight. */
function buildCardDecorations(
  model: SearchCardModel,
  highlight: SearchCardHighlight | null,
  docLineCount: number,
  lineFrom: (n: number) => { from: number; to: number },
): DecorationSet {
  const decos: { from: number; to: number; deco: Decoration }[] = [];
  if (highlight) {
    for (const t of highlight.tokens) {
      const typeName = highlight.typeNames[t.token_type];
      if (!typeName) continue;
      const cardLine = t.line + 2 - model.firstLine; // 0-based file → 1-based card
      if (cardLine < 1 || cardLine > docLineCount) continue;
      const line = lineFrom(cardLine);
      const from = line.from + t.start_char;
      const to = from + t.length;
      if (from < line.from || to > line.to) continue;
      decos.push({ from, to, deco: tokenDeco(typeName) });
    }
  }
  if (model.hit) {
    decos.push({ from: model.hit.from, to: model.hit.to, deco: hitDeco });
  }
  // RangeSetBuilder needs sorted input; the hit mark can overlap tokens, so
  // sort by (from, to) — CM renders overlapping marks as nested spans.
  decos.sort((a, b) => a.from - b.from || a.to - b.to);
  const builder = new RangeSetBuilder<Decoration>();
  for (const { from, to, deco } of decos) {
    if (to > from) builder.add(from, to, deco);
  }
  return builder.finish();
}

const setDecorations = StateEffect.define<DecorationSet>();

/** Holds the card's highlight set; mapped through edits between refreshes
 *  so colors track the text approximately until the file re-tokenizes. */
const decorationsField = StateField.define<DecorationSet>({
  create() {
    return Decoration.none;
  },
  update(value, tr) {
    let next = value.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(setDecorations)) next = e.value;
    }
    return next;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** Programmatic document reset (setCard) — not a user edit. */
const RESET = Annotation.define<boolean>();

// ── Buffer ──────────────────────────────────────────────────────────

export class SearchCardBuffer {
  private readonly host: HTMLElement;
  private readonly options: SearchCardBufferOptions;
  private readonly commitDelayMs: number;
  private readonly gutter = new Compartment();
  private view: EditorView | null = null;
  private model: SearchCardModel;
  private highlight: SearchCardHighlight | null;
  /** A user edit is awaiting commit. While set, setCard never resets the
   *  document (the user's text wins until it lands). */
  private dirty = false;
  private commitTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    host: HTMLElement,
    model: SearchCardModel,
    highlight: SearchCardHighlight | null,
    options: SearchCardBufferOptions,
  ) {
    this.host = host;
    this.options = options;
    this.commitDelayMs = options.commitDelayMs ?? DEFAULT_COMMIT_DELAY_MS;
    this.model = model;
    this.highlight = highlight;
    this.mount();
  }

  get editorView(): EditorView | null {
    return this.view;
  }

  /**
   * Feed a reconciled slice back in (snapshot remap / context change). When
   * the doc already shows the new text — the common case after the card's
   * own edit round-trips — only the metadata (offsets, gutter, highlight)
   * updates. A differing doc is reset only when no edit is pending.
   */
  setCard(model: SearchCardModel, highlight: SearchCardHighlight | null): void {
    const view = this.view;
    const gutterChanged = model.firstLine !== this.model.firstLine;
    this.model = model;
    this.highlight = highlight;
    if (!view) return;

    const effects: StateEffect<unknown>[] = [];
    if (gutterChanged) {
      effects.push(this.gutter.reconfigure(this.lineNumberGutter()));
    }

    if (view.state.doc.toString() !== model.text && !this.dirty) {
      const prev = view.state.selection.main;
      const len = model.text.length;
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: model.text },
        selection: EditorSelection.range(
          Math.min(prev.anchor, len),
          Math.min(prev.head, len),
        ),
        effects: [...effects, setDecorations.of(this.currentDecorations(model.text))],
        annotations: RESET.of(true),
      });
      return;
    }

    effects.push(setDecorations.of(this.currentDecorations(view.state.doc.toString())));
    view.dispatch({ effects, annotations: RESET.of(true) });
  }

  /** Tear down: flush pending commit, destroy the view, clear the host. */
  destroy(): void {
    this.flushPendingCommit();
    if (this.view) {
      this.view.destroy();
      this.view = null;
    }
    this.host.replaceChildren();
  }

  // ── Internal ─────────────────────────────────────────────────────

  private lineNumberGutter() {
    const offset = this.model.firstLine - 1;
    return lineNumbers({ formatNumber: (n) => String(n + offset) });
  }

  private currentDecorations(docText: string): DecorationSet {
    const lines = docText.split("\n");
    let acc = 0;
    const bounds = lines.map((text) => {
      const from = acc;
      acc += text.length + 1;
      return { from, to: from + text.length };
    });
    return buildCardDecorations(this.model, this.highlight, lines.length, (n) => {
      const b = bounds[n - 1];
      return b ?? { from: 0, to: 0 };
    });
  }

  private mount(): void {
    const listener = EditorView.updateListener.of((update) => {
      if (!update.docChanged) return;
      for (const tr of update.transactions) {
        if (tr.docChanged && !tr.annotation(RESET)) {
          this.onUserEdit();
          break;
        }
      }
    });

    const blur = EditorView.domEventHandlers({
      blur: () => {
        this.flushPendingCommit();
        return false;
      },
    });

    this.view = new EditorView({
      state: EditorState.create({
        doc: this.model.text,
        extensions: [
          this.gutter.of(this.lineNumberGutter()),
          brinkTheme,
          EditorView.lineWrapping,
          decorationsField.init(() => this.currentDecorations(this.model.text)),
          listener,
          blur,
        ],
      }),
      parent: this.host,
    });
  }

  private onUserEdit(): void {
    this.dirty = true;
    if (this.commitDelayMs <= 0) {
      this.flushPendingCommit();
      return;
    }
    if (this.commitTimer !== null) clearTimeout(this.commitTimer);
    this.commitTimer = setTimeout(() => {
      this.commitTimer = null;
      this.flushPendingCommit();
    }, this.commitDelayMs);
  }

  private flushPendingCommit(): void {
    if (this.commitTimer !== null) {
      clearTimeout(this.commitTimer);
      this.commitTimer = null;
    }
    if (!this.dirty || !this.view) return;
    this.dirty = false;
    const text = this.view.state.doc.toString();
    // A no-op round trip (edit + undo) writes nothing.
    if (text === this.model.text) return;
    this.options.onCommit(this.model.path, {
      start: this.model.from,
      end: this.model.to,
      text,
    });
  }
}

// ── Static rendering ────────────────────────────────────────────────

/** One run of a statically rendered card line. `classes` combines the
 *  semantic-token class (`tok-*`) and the hit mark, like the live buffer's
 *  decorations. */
export interface CardLineSegment {
  text: string;
  classes: string[];
}

/**
 * Per-line segments for a card rendered as static HTML (off-screen /
 * collapsed cards — the virtualization contract): the same token + hit
 * styling the live buffer shows, computed once from the cached tokens.
 */
export function cardLineSegments(
  model: SearchCardModel,
  highlight: SearchCardHighlight | null,
): CardLineSegment[][] {
  const lines = model.text.split("\n");
  let acc = 0;
  const out: CardLineSegment[][] = [];
  for (let i = 0; i < lines.length; i++) {
    const text = lines[i] ?? "";
    const lineFrom = acc;
    acc += text.length + 1;

    // Boundary sweep: token intervals on this line + the hit span.
    interface Interval {
      from: number;
      to: number;
      cls: string;
    }
    const intervals: Interval[] = [];
    if (highlight) {
      for (const t of highlight.tokens) {
        const cardLine = t.line + 2 - model.firstLine;
        if (cardLine !== i + 1) continue;
        const typeName = highlight.typeNames[t.token_type];
        if (!typeName) continue;
        const from = Math.max(0, t.start_char);
        const to = Math.min(text.length, t.start_char + t.length);
        if (to > from) intervals.push({ from, to, cls: "tok-" + typeName });
      }
    }
    if (model.hit) {
      const from = Math.max(0, model.hit.from - lineFrom);
      const to = Math.min(text.length, model.hit.to - lineFrom);
      if (to > from) intervals.push({ from, to, cls: "brink-search-hit" });
    }

    if (intervals.length === 0) {
      out.push(text.length > 0 ? [{ text, classes: [] }] : []);
      continue;
    }

    const points = new Set<number>([0, text.length]);
    for (const iv of intervals) {
      points.add(iv.from);
      points.add(iv.to);
    }
    const sorted = [...points].sort((a, b) => a - b);
    const segments: CardLineSegment[] = [];
    for (let p = 0; p < sorted.length - 1; p++) {
      const from = sorted[p] ?? 0;
      const to = sorted[p + 1] ?? 0;
      if (to <= from) continue;
      const classes = intervals.filter((iv) => iv.from <= from && iv.to >= to).map((iv) => iv.cls);
      segments.push({ text: text.slice(from, to), classes });
    }
    out.push(segments);
  }
  return out;
}
