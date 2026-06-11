/**
 * Search — project-wide find/replace tool window (issue #94, spec §4).
 *
 * The view is a thin surface over the store's search slice: it debounces
 * live search as the user types, renders results grouped by file
 * (collapsible headers), and dispatches `editor.reveal` with a source
 * Location for each row — navigation goes through the shared protocol
 * (§6.1) exactly like ProblemsView rows. Replacements run through the
 * slice, which reuses the binder structural-op path (updateFile +
 * invalidateFile + triggerCompile) so open editor views refresh.
 *
 * Replace-all is gated by an inline confirmation step (the acceptance
 * criterion "replace with confirmation"): the first click arms a confirm
 * bar with the match/file counts; only the explicit Replace click commits.
 *
 * Search state is transient (slice memory only) — query/options reset per
 * session; the tool window's placement persists like any other.
 */

import { memo, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  EDITOR_REVEAL_COMMAND_ID,
  ensureToolWindowOpen,
  useShell,
  type CommandRegistry,
  type ShellLayoutStore,
} from "@brink/studio-shell";
import {
  SEARCH_RESULT_CAP,
  matchLineSegments,
  type SearchMatch,
  type StudioStore,
} from "@brink/studio-store";
import { useStudioStore, useStudioStoreApi } from "./StoreContext.js";

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

interface FlatRow {
  path: string;
  match: SearchMatch;
}

function SearchViewInner() {
  const { commands } = useShell();
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
  const replaceMatch = useStudioStore((s) => s.replaceSearchMatch);
  const replaceAll = useStudioStore((s) => s.replaceAllSearchMatches);

  const [replaceOpen, setReplaceOpen] = useState(false);
  const [confirmingAll, setConfirmingAll] = useState(false);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [selected, setSelected] = useState(-1);
  const inputRef = useRef<HTMLInputElement>(null);

  // Live search, debounced while typing (also runs on mount, which simply
  // recomputes the current results against the live sources).
  useEffect(() => {
    const timer = setTimeout(() => runSearch(), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query, options, runSearch]);

  // search.focus → focus + select the query input (fires on mount too, so
  // opening the window by strip click or Mod-N also lands in the input).
  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focusSeq]);

  // New results disarm a pending replace-all confirmation and reset the
  // keyboard cursor — the confirmed counts must match what's on screen.
  useEffect(() => {
    setConfirmingAll(false);
    setSelected(-1);
  }, [results]);

  const flatRows = useMemo<FlatRow[]>(() => {
    if (results === null) return [];
    const rows: FlatRow[] = [];
    for (const file of results.files) {
      if (collapsed.has(file.path)) continue;
      for (const match of file.matches) rows.push({ path: file.path, match });
    }
    return rows;
  }, [results, collapsed]);

  const reveal = (path: string, match: SearchMatch): void => {
    commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
      kind: "source",
      file: path,
      span: { start: match.start, end: match.end },
    });
  };

  // Up/down + Enter from the query input walk the visible result rows.
  const onQueryKeyDown = (event: KeyboardEvent<HTMLInputElement>): void => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setSelected((i) => Math.min(i + 1, flatRows.length - 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setSelected((i) => Math.max(i - 1, 0));
    } else if (event.key === "Enter") {
      const row = flatRows[selected] ?? flatRows[0];
      if (row !== undefined) reveal(row.path, row.match);
    }
  };

  const toggleCollapsed = (path: string): void => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

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
  let rowIndex = -1;

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
                onKeyDown={onQueryKeyDown}
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
          <ul className="search-file-list">
            {results.files.map((file) => {
              const isCollapsed = collapsed.has(file.path);
              return (
                <li key={file.path} className="search-file">
                  <button
                    type="button"
                    className="search-file-header"
                    aria-expanded={!isCollapsed}
                    onClick={() => toggleCollapsed(file.path)}
                  >
                    <span
                      className={"search-chevron" + (isCollapsed ? " collapsed" : "")}
                    >
                      {"▶"}
                    </span>
                    <span className="search-file-path">{file.path}</span>
                    <span className="search-file-count">{file.matches.length}</span>
                  </button>
                  {!isCollapsed && (
                    <ul className="search-match-list">
                      {file.matches.map((match, i) => {
                        rowIndex++;
                        const isSelected = rowIndex === selected;
                        const segments = matchLineSegments(match);
                        return (
                          <li
                            key={`${match.start}:${i}`}
                            className={
                              "search-result-row" + (isSelected ? " selected" : "")
                            }
                          >
                            <button
                              type="button"
                              className="search-result-line"
                              title={`${file.path}:${match.line}`}
                              onClick={() => reveal(file.path, match)}
                            >
                              <span className="search-line-before">
                                {segments.before}
                              </span>
                              <mark className="search-line-match">
                                {segments.matchText}
                              </mark>
                              <span className="search-line-after">
                                {segments.after}
                              </span>
                            </button>
                            {replaceOpen && (
                              <button
                                type="button"
                                className="search-row-replace"
                                title="Replace this match"
                                aria-label="Replace this match"
                                onClick={() => replaceMatch(file.path, match)}
                              >
                                {"⇆"}
                              </button>
                            )}
                          </li>
                        );
                      })}
                    </ul>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

export const SearchView = memo(SearchViewInner);
