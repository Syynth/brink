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
 */

import type { StateCreator } from "zustand";
import type { StudioState } from "../index.js";
import {
  DEFAULT_SEARCH_OPTIONS,
  applyReplacements,
  buildSearchPattern,
  replacementTextFor,
  searchSources,
  type ProjectSearchResult,
  type ReplacementEdit,
  type SearchMatch,
  type SearchQueryOptions,
} from "@brink/ink-editor";

// ── Slice interface ─────────────────────────────────────────────────

export interface SearchSlice {
  searchQuery: string;
  searchOptions: SearchQueryOptions;
  searchReplace: string;
  /** Last search outcome; null = no search ran (empty query / regex error). */
  searchResults: ProjectSearchResult | null;
  /** Inline regex-validation error (like the Settings JSON error). */
  searchError: string | null;
  /** Bumped by `search.focus`; the view focuses its query input on change. */
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

  runSearch() {
    const { searchQuery, searchOptions, _project } = get();
    if (_project === null || searchQuery === "") {
      set({ searchResults: null, searchError: null });
      return;
    }
    const built = buildSearchPattern(searchQuery, searchOptions);
    if (!built.ok) {
      set({ searchResults: null, searchError: built.error });
      return;
    }
    const session = _project.getSession();
    // Sorted for deterministic file order (listFiles order is not a contract).
    const paths = session
      .listFiles()
      .map((f) => f.path)
      .sort();
    const sources: Array<{ path: string; source: string }> = [];
    for (const path of paths) {
      const source = session.getFileSource(path);
      if (source !== null) sources.push({ path, source });
    }
    set({ searchResults: searchSources(sources, built.pattern), searchError: null });
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
    // host egress, exactly like the binder structural-op path.
    project.applyEdit(
      path,
      applyReplacements(source, [{ start: match.start, end: match.end, text }]),
    );
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

    for (const { path, source, edits } of planned) {
      // Shared apply-edits seam (#137): see replaceSearchMatch.
      project.applyEdit(path, applyReplacements(source, edits));
      documents.invalidateFile(path);
    }
    documents.triggerCompile();
    get()._notify?.({
      severity: "info",
      source: "search",
      message: `Replaced ${plural(results.totalMatches, "match", "matches")} in ${plural(planned.length, "file", "files")}`,
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
    project.applyEdit(path, applyReplacements(source, [edit]));
    documents.invalidateFile(path);
    documents.triggerCompile();
    get().runSearch();
  },
});
