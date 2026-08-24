/**
 * SearchCardList — the per-match card surface of the Search panel
 * (docs/search-results-cards-spec.md, PR C; ruled 2026-08-24).
 *
 * One card per snapshot match, both modes (text search and references):
 * a header row (collapse chevron · file:line · containing knot/stitch ·
 * badges · reveal ↗) above the match's own small editable buffer — the
 * match line plus its context window, fully syntax-highlighted.
 *
 * The list is virtualized per the ruling ("if it's not too slow"): only
 * cards near the viewport mount a real CM6 editor ({@link SearchCardBuffer});
 * everything else renders the same content as static HTML from
 * {@link cardLineSegments}. Both share the **per-file token cache** — one
 * `getSearchHighlighting` call per (file, source), memoized here; cards
 * slice their lines from it. Never per-card wasm calls.
 *
 * The snapshot is frozen: a stale match keeps its card, badged `edited`,
 * with the hit mark dropped (the text no longer matches). Collapse state
 * lives in the store (per-card overrides + the all-flag, spanning modes).
 */

import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EDITOR_REVEAL_COMMAND_ID, useShell } from "@brink/studio-shell";
import {
  SearchCardBuffer,
  cardLineSegments,
  cardSlice,
  type CardSlice,
  type ReplacementEdit,
  type SearchCardHighlight,
  type SearchCardModel,
  type SnapshotMatch,
  type StudioState,
} from "@brink/studio-store";
import type { DocumentSymbol } from "@brink/wasm-types";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { containerAt } from "./TodosView.js";

// ── Card derivation ─────────────────────────────────────────────────

interface CardData {
  id: string;
  path: string;
  match: SnapshotMatch;
  container: string | null;
  model: SearchCardModel;
  fileDeleted: boolean;
  /** The file source the model's spans are valid against — the token
   *  cache's memo key. */
  seenSource: string;
}

function deriveCards(
  state: Pick<StudioState, "searchResults" | "searchContextLines" | "outline">,
): CardData[] {
  const snapshot = state.searchResults;
  if (snapshot === null) return [];
  const outlineByFile = new Map(state.outline.map((f) => [f.path, f.symbols]));
  const cards: CardData[] = [];
  for (const file of snapshot.files) {
    const symbols: readonly DocumentSymbol[] = outlineByFile.get(file.path) ?? [];
    for (const match of file.matches) {
      const slice: CardSlice = cardSlice(
        file.seenSource,
        match.start,
        match.end,
        state.searchContextLines,
      );
      cards.push({
        id: match.id,
        path: file.path,
        match,
        container: containerAt(symbols, match.start),
        fileDeleted: file.deleted,
        seenSource: file.seenSource,
        model: {
          path: file.path,
          from: slice.from,
          to: slice.to,
          firstLine: slice.firstLine,
          text: slice.text,
          // A stale match's text no longer satisfies the query — no hit mark
          // (the design's `edited` card shows the region unmarked).
          hit: match.stale ? null : slice.hit,
        },
      });
    }
  }
  return cards;
}

// ── Per-file token cache ────────────────────────────────────────────

/** Memoized per (path, source): ONE wasm tokenization per file with
 *  results; every card of that file (live or static) slices from it. */
function useHighlightCache(): (path: string, source: string) => SearchCardHighlight | null {
  const storeApi = useStudioStoreApi();
  const cacheRef = useRef(
    new Map<string, { source: string; data: SearchCardHighlight | null }>(),
  );
  return useCallback(
    (path: string, source: string) => {
      const cached = cacheRef.current.get(path);
      if (cached && cached.source === source) return cached.data;
      const data = storeApi.getState().getSearchHighlighting(path);
      cacheRef.current.set(path, { source, data });
      return data;
    },
    [storeApi],
  );
}

// ── Visibility (virtualization) ─────────────────────────────────────

/** True while `ref`'s element is within one viewport-margin of visible.
 *  Without IntersectionObserver (jsdom), every card counts as visible. */
function useNearViewport(ref: React.RefObject<HTMLElement | null>): boolean {
  const supported = typeof IntersectionObserver !== "undefined";
  const [visible, setVisible] = useState(!supported);
  useEffect(() => {
    if (!supported) return;
    const el = ref.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          setVisible(entry.isIntersecting);
        }
      },
      { rootMargin: "400px 0px" },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref, supported]);
  return visible;
}

// ── Static body ─────────────────────────────────────────────────────

function CardStaticBody({
  model,
  highlight,
}: {
  model: SearchCardModel;
  highlight: SearchCardHighlight | null;
}) {
  const lines = useMemo(() => cardLineSegments(model, highlight), [model, highlight]);
  return (
    <div className="search-card-static" aria-hidden="true">
      {lines.map((segments, i) => (
        <div className="search-card-static-line" key={i}>
          <span className="search-card-ln">{model.firstLine + i}</span>
          <span className="search-card-line-text">
            {segments.map((seg, j) =>
              seg.classes.length > 0 ? (
                <span key={j} className={seg.classes.join(" ")}>
                  {seg.text}
                </span>
              ) : (
                <span key={j}>{seg.text}</span>
              ),
            )}
          </span>
        </div>
      ))}
    </div>
  );
}

// ── Live (editable) body ────────────────────────────────────────────

function CardEditorBody({
  card,
  highlight,
}: {
  card: CardData;
  highlight: SearchCardHighlight | null;
}) {
  const storeApi = useStudioStoreApi();
  const hostRef = useRef<HTMLDivElement>(null);
  const bufferRef = useRef<SearchCardBuffer | null>(null);
  // Latest model/highlight in refs so the mount effect stays stable.
  const cardRef = useRef(card);
  cardRef.current = card;
  const highlightRef = useRef(highlight);
  highlightRef.current = highlight;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const buffer = new SearchCardBuffer(host, cardRef.current.model, highlightRef.current, {
      onCommit: (path: string, edit: ReplacementEdit) => {
        const state = storeApi.getState();
        // The edit's offsets are valid against the snapshot's seenSource;
        // only write when the live text still carries the original slice
        // (otherwise the next remap rebuilds the card and nothing is lost
        // but this one keystroke burst — never a corrupted write).
        const live = state.getSearchSource(path);
        if (live === null) return;
        if (live.slice(edit.start, edit.end) !== cardRef.current.model.text) return;
        state.applySearchRowEdit(path, edit);
      },
    });
    bufferRef.current = buffer;

    // e2e / manual-verification hook (CM6 renders only the viewport): the
    // card's EditorView keyed by its stable match id.
    const w = window as unknown as { __brinkSearchCardViews?: Record<string, unknown> };
    w.__brinkSearchCardViews ??= {};
    const id = cardRef.current.id;
    w.__brinkSearchCardViews[id] = buffer.editorView ?? undefined;

    return () => {
      if (w.__brinkSearchCardViews?.[id] === buffer.editorView) {
        delete w.__brinkSearchCardViews[id];
      }
      buffer.destroy();
      bufferRef.current = null;
    };
  }, [storeApi]);

  // Reconcile snapshot remaps / context changes into the live buffer.
  useEffect(() => {
    bufferRef.current?.setCard(card.model, highlight);
  }, [card.model, highlight]);

  return <div className="search-card-editor" ref={hostRef} />;
}

// ── Card ────────────────────────────────────────────────────────────

/** Collapsed-header preview: the match line with the hit marked. */
function CollapsedPreview({ match }: { match: SnapshotMatch }) {
  const { lineText, lineStart, lineEnd, stale } = match;
  if (stale) return <span className="search-card-preview">{lineText}</span>;
  return (
    <span className="search-card-preview">
      {lineText.slice(0, lineStart)}
      <span className="brink-search-hit">{lineText.slice(lineStart, lineEnd)}</span>
      {lineText.slice(lineEnd)}
    </span>
  );
}

function SearchCard({
  card,
  getHighlight,
}: {
  card: CardData;
  getHighlight: (path: string, source: string) => SearchCardHighlight | null;
}) {
  const { commands } = useShell();
  const collapsedMap = useStudioStore((s) => s.searchCardCollapsed);
  const allCollapsed = useStudioStore((s) => s.searchAllCollapsed);
  const setCollapsed = useStudioStore((s) => s.setSearchCardCollapsed);
  const collapsed = collapsedMap[card.id] ?? allCollapsed;

  const rootRef = useRef<HTMLDivElement>(null);
  const near = useNearViewport(rootRef);
  // Keyed by the file's seenSource — every card of a file shares one entry.
  const highlight = collapsed ? null : getHighlight(card.path, card.seenSource);

  const reveal = () => {
    commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
      kind: "source",
      file: card.path,
      span: { start: card.match.start, end: card.match.end },
    });
  };

  const edited = card.match.edited || card.match.stale;
  return (
    <div
      ref={rootRef}
      className={"search-card" + (card.match.stale ? " stale" : "")}
      data-card-id={card.id}
    >
      <div className="search-card-head">
        <button
          type="button"
          className="search-card-chevron"
          aria-expanded={!collapsed}
          aria-label={collapsed ? "Expand card" : "Collapse card"}
          onClick={() => setCollapsed(card.id, !collapsed)}
        >
          {collapsed ? "›" : "⌄"}
        </button>
        <span className="search-card-loc">
          {card.path}:{card.match.line}
        </span>
        {card.container !== null && (
          <>
            <span className="search-card-sep">·</span>
            <span className="search-card-container">{card.container}</span>
          </>
        )}
        {card.fileDeleted && <span className="search-card-badge deleted">deleted</span>}
        {!card.fileDeleted && edited && (
          <span
            className="search-card-badge edited"
            title="Edited — no longer matches. Kept: results are a snapshot."
          >
            edited
          </span>
        )}
        {collapsed && <CollapsedPreview match={card.match} />}
        <button
          type="button"
          className="search-card-reveal"
          title="Reveal in editor"
          aria-label={`Reveal ${card.path}:${card.match.line} in editor`}
          onClick={reveal}
        >
          ↗
        </button>
      </div>
      {!collapsed &&
        (near && !card.fileDeleted ? (
          <CardEditorBody card={card} highlight={highlight} />
        ) : (
          <CardStaticBody model={card.model} highlight={highlight} />
        ))}
    </div>
  );
}

// ── List ────────────────────────────────────────────────────────────

function SearchCardListInner() {
  const results = useStudioStore((s) => s.searchResults);
  const contextLines = useStudioStore((s) => s.searchContextLines);
  const outline = useStudioStore((s) => s.outline);
  const getHighlight = useHighlightCache();
  const cards = useMemo(
    () => deriveCards({ searchResults: results, searchContextLines: contextLines, outline }),
    [results, contextLines, outline],
  );
  return (
    <div className="search-cards">
      {cards.map((card) => (
        <SearchCard key={card.id} card={card} getHighlight={getHighlight} />
      ))}
    </div>
  );
}

export const SearchCardList = memo(SearchCardListInner);
