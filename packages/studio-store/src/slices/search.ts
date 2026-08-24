/**
 * Search slice — project-wide find/replace state (issue #94, spec §4).
 *
 * Transient by design: query, options, and results live only in memory and
 * reset per session (nothing persists). The view debounces `runSearch`;
 * the slice itself is synchronous — it reads the wasm session's current
 * file sources through `_project` at call time, so results always reflect
 * the live (possibly unsaved) buffers.
 *
 * Replacements reuse the binder structural-op path exactly (see
 * binder.ts:applyMoveResult): `session.updateFile` rewrites the source,
 * `documents.invalidateFile` refreshes every mounted CM6 view of the file,
 * and `documents.triggerCompile` refreshes outline/diagnostics. Stale
 * results (the file changed since the search ran) are detected by
 * re-checking each match's text against the live source — nothing is
 * replaced on a mismatch; the search re-runs instead.
 *
 * Results are a **frozen snapshot** (docs/search-results-cards-spec.md,
 * ruled 2026-08-24): `searchResults` holds a `SearchSnapshot` whose match
 * spans are edit-mapped through document changes (`remapSearchSnapshot`,
 * driven by the compile seam — every edit path funnels through a compile).
 * Edits flag rows `edited`/`stale`; they never remove them. Only a new
 * search, `showReferences`, or the explicit `refreshSearchSnapshot` (the
 * panel's ↻) replaces the set. `SearchSnapshot` is a structural superset
 * of `ProjectSearchResult`, so existing consumers read it unchanged.
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import type { ProjectSession } from "../types.js";
import {
  DEFAULT_SEARCH_OPTIONS,
  applyReplacements,
  buildSearchPattern,
  locationsToSearchResult,
  replacementTextFor,
  searchSources,
  type ReplacementEdit,
  type SearchMatch,
  type SearchQueryOptions,
} from "@brink-lang/editor";
import {
  DEFAULT_SEARCH_CONTEXT_LINES,
  captureSnapshot,
  clampContextLines,
  remapSnapshot,
  type SearchContextLines,
  type SearchSnapshot,
  type SnapshotOrigin,
} from "../search-snapshot.js";

// ── Slice interface ─────────────────────────────────────────────────

export interface SearchSlice {
  /** What the results currently show: a text query, or a symbol's
   *  references (context-menu spec ruling: the Search panel is the
   *  references surface). Typing a query returns the panel to query mode. */
  searchMode: { kind: "query" } | { kind: "references"; symbol: string };
  /** Populate the panel with a symbol's references (grouped like search
   *  results; replace controls are inert in this mode). `declaration` is
   *  the symbol's definition location when the caller resolved one — it is
   *  edit-mapped as the snapshot's anchor so ↻ re-resolves from the
   *  declaration's *current* position. */
  showReferences(
    symbol: string,
    locations: { file: string; start: number; end: number }[],
    declaration?: { file: string; start: number; end: number } | null,
  ): void;
  /** Bumped by showReferences — <SearchCommands/> reacts by ensuring the
   *  Search tool window is open (the layout store lives in the shell,
   *  unreachable from the slice). */
  searchRevealSeq: number;
  /** Leave references mode (the chip's ✕): clears the results; the typed
   *  query (if any) is left alone and not re-run. */
  clearReferences(): void;

  searchQuery: string;
  searchOptions: SearchQueryOptions;
  searchReplace: string;
  /** Last search outcome; null = no search ran (empty query / regex error).
   *  A frozen, edit-mapped snapshot — see the module doc. */
  searchResults: SearchSnapshot | null;
  /** Inline regex-validation error (like the Settings JSON error). */
  searchError: string | null;
  /**
   * Bumped by `search.focus`; the view focuses its query input on change.
   *
   * Invariant (docs/studio-shell-spec.md §7.7.1, #2527): this is advanced
   * *only* in response to a user invoking Find in Files. The view also
   * `select()`s the query on every change, unguarded, which is correct
   * precisely because a bump means "the user asked to replace this query" —
   * bumping it from a path the user did not initiate (results arriving, a
   * project reload) would select the text mid-typing and lose the next
   * keystroke. `packages/brink-studio/src/__tests__/search-view-focus.test.tsx`
   * fails if a new non-user-initiated caller appears.
   */
  searchFocusSeq: number;

  setSearchQuery(query: string): void;
  toggleSearchOption(option: keyof SearchQueryOptions): void;
  setSearchReplace(text: string): void;
  requestSearchFocus(): void;
  /** Search the live session sources now (the view debounces calls). */
  runSearch(): void;
  /** Replace one result (stale matches refresh the search instead). */
  replaceSearchMatch(path: string, match: SearchMatch): void;
  /** Replace every listed result. All-or-nothing on stale results. */
  replaceAllSearchMatches(): void;

  // ── Snapshot model (docs/search-results-cards-spec.md, PR B) ──────

  /** Card context window (lines above/below the match line; default 1/2).
   *  Session-transient like everything else in this slice. */
  searchContextLines: SearchContextLines;
  setSearchContextLines(lines: SearchContextLines): void;
  /** Per-card collapse overrides, keyed by `SnapshotMatch.id`. A card
   *  without an entry follows `searchAllCollapsed`. Cleared when a new
   *  snapshot replaces the set (ids restart); the all-flag survives across
   *  snapshots and modes ("collapse state spans modes"). */
  searchCardCollapsed: Readonly<Record<string, boolean>>;
  searchAllCollapsed: boolean;
  setSearchCardCollapsed(id: string, collapsed: boolean): void;
  /** The summary row's collapse-all/expand-all: sets the default and drops
   *  every per-card override. */
  setAllSearchCardsCollapsed(collapsed: boolean): void;
  /** Re-map every snapshot span through whatever changed since the last
   *  map, refreshing `edited`/`stale`. Called from the compile seam
   *  (`setCompileResult`) — every edit path funnels through a compile. */
  remapSearchSnapshot(): void;
  /** The panel's ↻: re-run the snapshot's own origin. Query snapshots
   *  re-run their frozen query (not the input field's current text);
   *  references snapshots re-resolve from the edit-mapped declaration
   *  anchor (no anchor → no-op). */
  refreshSearchSnapshot(): void;

  // ── Editable results buffer (#322 Track V, design D) ──────────────

  /** Live source of a file, for the results buffer's stale/skip guard. Null
   *  when no project is loaded or the file is gone. */
  getSearchSource(path: string): string | null;
  /**
   * Apply an edit the user made in the editable results buffer, routed to the
   * source through the shared apply-edits seam (updateFile + invalidate +
   * compile), then re-run the search so the buffer reflects the new sources.
   */
  applySearchRowEdit(path: string, edit: ReplacementEdit): void;
}

// ── Helpers ─────────────────────────────────────────────────────────

function plural(n: number, singular: string, pluralForm: string): string {
  return `${n} ${n === 1 ? singular : pluralForm}`;
}

/** The wasm session surface this slice reads (structural — test fakes
 *  implement just what they exercise). */
type SessionLike = ReturnType<ProjectSession["getSession"]>;

/**
 * Run a text query over the live session sources and freeze the outcome
 * into a snapshot. Null when the pattern does not compile (callers that
 * validated it first never see null).
 *
 * Sorted for deterministic file order (listFiles order is not a contract).
 * Excludes mounted stdlib files (issue #2306/#2343, "Excluded from
 * save-all and search/replace"): `listFiles()` now lists them alongside
 * real project files (flagged `mounted`, #2343's flag flip) so the
 * Binder's Library section has something to render, but project-wide
 * search must keep treating the library as out of scope — searching
 * into it would surface matches the replace path (`applyEdit`) then has
 * to silently skip anyway.
 */
function captureQuerySnapshot(
  session: SessionLike,
  query: string,
  options: SearchQueryOptions,
): SearchSnapshot | null {
  const built = buildSearchPattern(query, options);
  if (!built.ok) return null;
  const paths = session
    .listFiles()
    .filter((f) => !f.mounted)
    .map((f) => f.path)
    .sort();
  const sources: Array<{ path: string; source: string }> = [];
  const byPath = new Map<string, string>();
  for (const path of paths) {
    const source = session.getFileSource(path);
    if (source !== null) {
      sources.push({ path, source });
      byPath.set(path, source);
    }
  }
  const result = searchSources(sources, built.pattern);
  // Capture against the exact sources just searched (not a re-read) so the
  // snapshot baseline and the match spans cannot disagree.
  return captureSnapshot(result, { kind: "query", query, options }, (p) => byPath.get(p) ?? null);
}

// ── Slice creator ───────────────────────────────────────────────────

export const createSearchSlice: StateCreator<StudioState, [], [], SearchSlice> = (
  set,
  get,
) => ({
  searchQuery: "",
  searchOptions: DEFAULT_SEARCH_OPTIONS,
  searchReplace: "",
  searchResults: null,
  searchError: null,
  searchFocusSeq: 0,

  setSearchQuery(query) {
    set({ searchQuery: query });
  },

  toggleSearchOption(option) {
    const options = get().searchOptions;
    set({ searchOptions: { ...options, [option]: !options[option] } });
  },

  setSearchReplace(text) {
    set({ searchReplace: text });
  },

  requestSearchFocus() {
    set({ searchFocusSeq: get().searchFocusSeq + 1 });
  },

  // ── Snapshot model (docs/search-results-cards-spec.md, PR B) ──────

  searchContextLines: DEFAULT_SEARCH_CONTEXT_LINES,
  searchCardCollapsed: {},
  searchAllCollapsed: false,

  setSearchContextLines(lines) {
    set({ searchContextLines: clampContextLines(lines) });
  },

  setSearchCardCollapsed(id, collapsed) {
    set({ searchCardCollapsed: { ...get().searchCardCollapsed, [id]: collapsed } });
  },

  setAllSearchCardsCollapsed(collapsed) {
    set({ searchAllCollapsed: collapsed, searchCardCollapsed: {} });
  },

  remapSearchSnapshot() {
    const { searchResults, _project } = get();
    if (searchResults === null || _project === null) return;
    const session = _project.getSession();
    const remapped = remapSnapshot(searchResults, (path) => session.getFileSource(path));
    // remapSnapshot returns the same object when nothing moved — skip the
    // store update so subscribers don't re-render on every compile.
    if (remapped !== searchResults) set({ searchResults: remapped });
  },

  refreshSearchSnapshot() {
    const { searchResults, _project } = get();
    if (searchResults === null || _project === null) return;
    const session = _project.getSession();
    if (searchResults.origin.kind === "query") {
      // Re-run the snapshot's own frozen query — not the input field's
      // current text (live search already owns that path).
      const snapshot = captureQuerySnapshot(
        session,
        searchResults.origin.query,
        searchResults.origin.options,
      );
      if (snapshot !== null) {
        set({ searchResults: snapshot, searchError: null, searchCardCollapsed: {} });
      }
      return;
    }
    // References: map the anchor through any edits since the last
    // compile-driven remap, then re-resolve from its *current* position
    // (the original click offset goes stale — spec ruling).
    get().remapSearchSnapshot();
    const current = get().searchResults;
    if (current === null || current.origin.kind !== "references") return;
    const anchor = current.anchor;
    if (anchor === null) return;
    let locations: { file: string; start: number; end: number }[];
    try {
      locations = session.findReferencesAt(anchor.file, anchor.start, true);
    } catch {
      // Resolution failed (symbol gone, project mid-edit) — keep the
      // existing snapshot rather than blanking the panel.
      return;
    }
    const getSource = (path: string): string | null => session.getFileSource(path);
    const results = locationsToSearchResult(locations, getSource);
    set({
      searchResults: captureSnapshot(
        results,
        { kind: "references", symbol: current.origin.symbol },
        getSource,
        anchor,
      ),
      searchCardCollapsed: {},
    });
  },

  searchMode: { kind: "query" },
  searchRevealSeq: 0,

  clearReferences() {
    if (get().searchMode.kind !== "references") return;
    set({ searchResults: null, searchMode: { kind: "query" }, searchCardCollapsed: {} });
  },

  showReferences(symbol, locations, declaration = null) {
    const project = get()._project;
    if (project === null) return;
    const session = project.getSession();
    const getSource = (path: string): string | null => session.getFileSource(path);
    const results = locationsToSearchResult(locations, getSource);
    set({
      searchResults: captureSnapshot(results, { kind: "references", symbol }, getSource, declaration),
      searchError: null,
      searchMode: { kind: "references", symbol },
      searchRevealSeq: get().searchRevealSeq + 1,
      searchCardCollapsed: {},
    });
  },

  runSearch() {
    const { searchQuery, searchOptions, _project } = get();
    if (_project === null || searchQuery === "") {
      set({
        searchResults: null,
        searchError: null,
        searchMode: { kind: "query" },
        searchCardCollapsed: {},
      });
      return;
    }
    const built = buildSearchPattern(searchQuery, searchOptions);
    if (!built.ok) {
      set({
        searchResults: null,
        searchError: built.error,
        searchMode: { kind: "query" },
        searchCardCollapsed: {},
      });
      return;
    }
    set({
      searchResults: captureQuerySnapshot(_project.getSession(), searchQuery, searchOptions),
      searchError: null,
      searchMode: { kind: "query" },
      // A new snapshot restarts card identity; per-card collapse overrides
      // die with the old ids (the all-flag survives — "spans modes").
      searchCardCollapsed: {},
    });
  },

  replaceSearchMatch(path, match) {
    const state = get();
    const project = state._project;
    const documents = state._documents;
    if (!project || !documents) return;
    const built = buildSearchPattern(state.searchQuery, state.searchOptions);
    if (!built.ok) return;

    const session = project.getSession();
    const source = session.getFileSource(path);
    if (source === null || source.slice(match.start, match.end) !== match.text) {
      // Stale result — the file changed underneath. Refresh, replace nothing.
      get().runSearch();
      return;
    }

    const text = replacementTextFor(
      match,
      built.pattern,
      state.searchReplace,
      state.searchOptions.regex,
    );
    // Through the shared apply-edits seam (#137): provider write-back +
    // host egress, exactly like the binder structural-op path. `applyEdit`
    // refuses a mounted stdlib path (issue #2306) — `listFiles`-derived
    // results never include one today, but a caller reaching this with a
    // by-id path outside that listing (the exact hole #2306 closes) must
    // not silently fork the library.
    const applied = project.applyEdit(
      path,
      applyReplacements(source, [{ start: match.start, end: match.end, text }]),
    );
    if (!applied) {
      get()._notify?.({
        severity: "warning",
        source: "search",
        message: `"${path}" is part of the read-only library and cannot be edited`,
      });
      return;
    }
    documents.invalidateFile(path);
    documents.triggerCompile();
    get().runSearch();
  },

  replaceAllSearchMatches() {
    const state = get();
    const project = state._project;
    const documents = state._documents;
    const results = state.searchResults;
    if (!project || !documents || results === null || results.totalMatches === 0) {
      return;
    }
    const built = buildSearchPattern(state.searchQuery, state.searchOptions);
    if (!built.ok) return;

    const session = project.getSession();

    // Plan every edit first — no partial replace over a stale result set.
    const planned: Array<{ path: string; source: string; edits: ReplacementEdit[] }> =
      [];
    let stale = false;
    for (const file of results.files) {
      const source = session.getFileSource(file.path);
      if (source === null) {
        stale = true;
        break;
      }
      const edits: ReplacementEdit[] = [];
      for (const match of file.matches) {
        if (source.slice(match.start, match.end) !== match.text) {
          stale = true;
          break;
        }
        edits.push({
          start: match.start,
          end: match.end,
          text: replacementTextFor(
            match,
            built.pattern,
            state.searchReplace,
            state.searchOptions.regex,
          ),
        });
      }
      if (stale) break;
      planned.push({ path: file.path, source, edits });
    }

    if (stale) {
      get().runSearch();
      get()._notify?.({
        severity: "warning",
        source: "search",
        message:
          "The project changed since the search ran — results refreshed, nothing replaced",
      });
      return;
    }

    // Shared apply-edits seam (#137): see replaceSearchMatch. `applyEdit`
    // refuses a mounted stdlib path (issue #2306) — skipped files are
    // reported alongside the replace count rather than silently dropped.
    let skipped = 0;
    let filesChanged = 0;
    let matchesReplaced = 0;
    for (const { path, source, edits } of planned) {
      if (!project.applyEdit(path, applyReplacements(source, edits))) {
        skipped += 1;
        continue;
      }
      documents.invalidateFile(path);
      filesChanged += 1;
      matchesReplaced += edits.length;
    }
    documents.triggerCompile();
    const skippedSuffix =
      skipped > 0 ? ` (skipped ${plural(skipped, "read-only file", "read-only files")})` : "";
    get()._notify?.({
      severity: "info",
      source: "search",
      message: `Replaced ${plural(matchesReplaced, "match", "matches")} in ${plural(filesChanged, "file", "files")}${skippedSuffix}`,
    });
    get().runSearch();
  },

  getSearchSource(path) {
    const project = get()._project;
    if (!project) return null;
    return project.getSession().getFileSource(path);
  },

  applySearchRowEdit(path, edit) {
    const state = get();
    const project = state._project;
    const documents = state._documents;
    if (!project || !documents) return;

    const source = project.getSession().getFileSource(path);
    if (source === null) {
      // File vanished underneath — refresh, apply nothing.
      get().runSearch();
      return;
    }

    // Through the shared apply-edits seam (#137), exactly like the tree's
    // per-row replace: provider write-back + host egress + view refresh.
    // `applyEdit` refuses a mounted stdlib path (issue #2306).
    if (!project.applyEdit(path, applyReplacements(source, [edit]))) {
      get()._notify?.({
        severity: "warning",
        source: "search",
        message: `"${path}" is part of the read-only library and cannot be edited`,
      });
      return;
    }
    documents.invalidateFile(path);
    documents.triggerCompile();
    get().runSearch();
  },
});
