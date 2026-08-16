/**
 * Search — project-wide find/replace tool window (issue #94, spec §4;
 * editable results buffer #322 Track V).
 *
 * The view is a thin surface over the store's search slice: it debounces
 * live search as the user types and renders the results through the
 * editor-owned *editable* results buffer ({@link SearchResultsBufferView} —
 * the locked Zed-style design D): a synthetic CodeMirror document mirroring
 * the cross-file matches (file headers + match lines), where editing a match
 * row routes the change back to the source document through the shared
 * apply-edits seam. Double-clicking a match row dispatches `editor.reveal`
 * exactly like the old tree rows.
 *
 * Replace-all is gated by an inline confirmation step (the acceptance
 * criterion "replace with confirmation"): the first click arms a confirm
 * bar with the match/file counts; only the explicit Replace click commits.
 *
 * Search state is transient (slice memory only) — query/options reset per
 * session; the tool window's placement persists like any other.
 */

import { memo, useEffect, useRef, useState } from "react";
import {
  ensureToolWindowOpen,
  useShell,
  type CommandRegistry,
  type ShellLayoutStore,
} from "@brink/studio-shell";
import { SEARCH_RESULT_CAP, type StudioStore } from "@brink/studio-store";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";
import { SearchResultsBufferView } from "./SearchResultsBufferView.js";

export const SEARCH_TOOL_WINDOW_ID = "search";
export const SEARCH_FOCUS_COMMAND_ID = "search.focus";

/** Live-search debounce while typing in the query input. */
export const SEARCH_DEBOUNCE_MS = 200;

// ── Command ─────────────────────────────────────────────────────────

/**
 * Register `search.focus` (palette: "Search: Find in Files", Mod-Shift-F —
 * VS Code precedent; free in the shell keymap and unclaimed by CM6's
 * default/search keymaps). Opens the tool window without ever closing it
 * (ensureToolWindowOpen, not toggle) and asks the view to focus its query
 * input via the slice's focus sequence.
 */
export function registerSearchFocusCommand(
  commands: CommandRegistry,
  layout: ShellLayoutStore,
  store: StudioStore,
): () => void {
  return commands.register({
    id: SEARCH_FOCUS_COMMAND_ID,
    title: "Search: Find in Files",
    keybinding: "Mod-Shift-F",
    run: () => {
      ensureToolWindowOpen(layout, SEARCH_TOOL_WINDOW_ID);
      store.getState().requestSearchFocus();
    },
  });
}

/**
 * Mounts the search.focus command. Rendered by App (always mounted, like
 * QuickOpen's self-registration) because the command needs the shell's
 * layout store, which only exists inside ShellProvider — while SearchView
 * itself is unmounted whenever the tool window is closed.
 */
export function SearchCommands() {
  const { commands, layout } = useShell();
  const store = useStudioStoreApi();
  useEffect(
    () => registerSearchFocusCommand(commands, layout, store),
    [commands, layout, store],
  );
  return null;
}

// ── View ────────────────────────────────────────────────────────────

function SearchViewInner() {
  const query = useStudioStore((s) => s.searchQuery);
  const options = useStudioStore((s) => s.searchOptions);
  const replaceText = useStudioStore((s) => s.searchReplace);
  const results = useStudioStore((s) => s.searchResults);
  const error = useStudioStore((s) => s.searchError);
  const focusSeq = useStudioStore((s) => s.searchFocusSeq);
  const setQuery = useStudioStore((s) => s.setSearchQuery);
  const toggleOption = useStudioStore((s) => s.toggleSearchOption);
  const setReplace = useStudioStore((s) => s.setSearchReplace);
  const runSearch = useStudioStore((s) => s.runSearch);
  const replaceAll = useStudioStore((s) => s.replaceAllSearchMatches);

  const [replaceOpen, setReplaceOpen] = useState(false);
  const [confirmingAll, setConfirmingAll] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  // Live search, debounced while typing (also runs on mount, which simply
  // recomputes the current results against the live sources).
  useEffect(() => {
    const timer = setTimeout(() => runSearch(), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query, options, runSearch]);

  // search.focus → focus + select the query input (fires on mount too, so
  // opening the window by strip click or its Mod-6 toggle also lands in the
  // input).
  //
  // The `select()` is deliberately unguarded, which is the opposite of the
  // rule for the studio's other text inputs (docs/studio-shell-spec.md
  // §7.7.1): elsewhere an unguarded `select()` primes the next keystroke to
  // replace text the user typed, which is how #2511 lost renames. Two
  // properties make it correct — and safe — here, and only here:
  //
  //   1. The query field is *controlled* (`value={query}` from the store, and
  //      `onChange` writes straight back). Nothing ever assigns `input.value`,
  //      and this effect is a plain `useEffect` body rather than a deferred
  //      frame, so there is no window in which the DOM and the store disagree
  //      and nothing to seed over.
  //   2. `focusSeq` advances only from `requestSearchFocus()`, whose sole
  //      caller is the `search.focus` command's `run` above — reachable only
  //      via Mod-Shift-F or the palette. Re-invoking Find in Files means
  //      "replace this query", so selecting it is the intent (VS Code
  //      precedent), not a clobber. A mount cannot clobber either: it builds a
  //      fresh input, so no keystroke of the user's exists in it yet.
  //
  // Property 2 is load-bearing. Raising a focus request from a path the user
  // did not initiate — results arriving, a project reload, a focus-restore
  // effect — would fire this `select()` mid-typing and make it a real
  // input-loss bug. `packages/brink-studio/src/__tests__/search-view-focus.test.tsx`
  // fails if that property is broken, if the dependency list is widened so
  // this runs on unrelated re-renders, or if the intended select-on-invoke is
  // removed.
  useEffect(() => {
    inputRef.current?.focus();
    // SELECT-INVARIANT SearchView.select: the query field is controlled
    // (rule 1 holds by construction) and this effect runs only on mount or
    // an explicit search.focus invocation (property 2 above) — "replace this
    // query" is the correct reading of that command, not a clobber. See the
    // comment block above this effect and search-view-focus.test.tsx.
    inputRef.current?.select();
  }, [focusSeq]);

  // New results disarm a pending replace-all confirmation — the confirmed
  // counts must match what's on screen.
  useEffect(() => {
    setConfirmingAll(false);
  }, [results]);

  const optionButton = (
    key: "caseSensitive" | "wholeWord" | "regex",
    label: string,
    title: string,
  ) => (
    <button
      type="button"
      className={"search-option" + (options[key] ? " active" : "")}
      aria-pressed={options[key]}
      title={title}
      data-option={key}
      onClick={() => toggleOption(key)}
    >
      {label}
    </button>
  );

  const total = results?.totalMatches ?? 0;
  const fileCount = results?.files.length ?? 0;

  return (
    <div className="search-view">
      <div className="search-form">
        <div className="search-query-block">
          <button
            type="button"
            className="search-replace-toggle"
            aria-label="Toggle replace"
            aria-expanded={replaceOpen}
            title="Toggle replace"
            onClick={() => setReplaceOpen((open) => !open)}
          >
            <span className={"search-chevron" + (replaceOpen ? "" : " collapsed")}>
              {"▶"}
            </span>
          </button>
          <div className="search-fields">
            <div className="search-input-row">
              <input
                ref={inputRef}
                className="search-input"
                value={query}
                placeholder="Search"
                spellCheck={false}
                aria-label="Search query"
                onChange={(event) => setQuery(event.target.value)}
              />
              <div className="search-options" role="group" aria-label="Search options">
                {optionButton("caseSensitive", "Aa", "Match case")}
                {optionButton("wholeWord", "ab", "Match whole word")}
                {optionButton("regex", ".*", "Use regular expression")}
              </div>
            </div>
            {replaceOpen && (
              <div className="search-input-row">
                <input
                  className="search-replace-input"
                  value={replaceText}
                  placeholder="Replace"
                  spellCheck={false}
                  aria-label="Replace text"
                  onChange={(event) => setReplace(event.target.value)}
                />
                <button
                  type="button"
                  className="search-replace-all"
                  disabled={total === 0}
                  title="Replace all matches"
                  onClick={() => setConfirmingAll(true)}
                >
                  Replace All
                </button>
              </div>
            )}
          </div>
        </div>
        {error !== null && (
          <p className="search-error" role="alert">
            {error}
          </p>
        )}
        {confirmingAll && total > 0 && (
          <div className="search-confirm" role="alertdialog" aria-label="Confirm replace all">
            <span className="search-confirm-text">
              Replace {total} {total === 1 ? "match" : "matches"} in {fileCount}{" "}
              {fileCount === 1 ? "file" : "files"}?
            </span>
            <button
              type="button"
              className="search-confirm-yes"
              onClick={() => {
                setConfirmingAll(false);
                replaceAll();
              }}
            >
              Replace
            </button>
            <button
              type="button"
              className="search-confirm-no"
              onClick={() => setConfirmingAll(false)}
            >
              Cancel
            </button>
          </div>
        )}
      </div>

      <div className="search-results">
        {results !== null && total === 0 && (
          <p className="search-empty">No results</p>
        )}
        {results !== null && results.capped && (
          <p className="search-capped" role="status">
            Results capped at {SEARCH_RESULT_CAP} matches — refine the query
          </p>
        )}
        {results !== null && total > 0 && (
          // Editor-owned editable results buffer (#322 Track V, design D):
          // headers + match lines in a scrollable CM6 document; editing a
          // match row routes back to the source. Keyed on the total so a
          // structural change (files added/removed between searches) remounts
          // cleanly; in-place edits keep the same view via setResult.
          <SearchResultsBufferView key={fileCount} results={results} />
        )}
      </div>
    </div>
  );
}

export const SearchView = memo(SearchViewInner);
