/**
 * SearchResultsBufferView — the editable results buffer surface (issue #322,
 * Track V, design D — the locked Zed-style ask).
 *
 * Replaces the read-only match tree with a single synthetic CodeMirror
 * document whose lines mirror the cross-file search results (file headers +
 * match lines with line numbers). Editing a match row rewrites the source
 * line in the underlying document, routed back through the shared apply-edits
 * seam (`ProjectSession.applyEdit`) by the store's `applySearchRowEdit`.
 *
 * The framework-agnostic {@link SearchResultsBuffer} (in @brink-lang/editor)
 * owns the EditorView, the read-only filter (only match-line source columns
 * are editable), and the edit→source mapping. This component only mounts it
 * into a host container for its lifetime and feeds it fresh results via
 * `setResult`, tearing it down on unmount — `SearchResultsBuffer.destroy()`
 * removes the view + every listener (CM6 teardown contract; leaks are bugs).
 *
 * Double-click a match row — or focus it and press Enter / Mod-Enter — reveals
 * it in the normal editor through the shared `editor.reveal` command, exactly
 * like the tree rows did (and keyboard-reachable, unlike the tree's buttons).
 */

import { useEffect, useRef } from "react";
import { EDITOR_REVEAL_COMMAND_ID, useShell } from "@brink/studio-shell";
import { SearchResultsBuffer, type ProjectSearchResult } from "@brink/studio-store";
import { useStudioStoreApi } from "./StoreContext.js";

export function SearchResultsBufferView({ results }: { results: ProjectSearchResult }) {
  const storeApi = useStudioStoreApi();
  const { commands } = useShell();
  const hostRef = useRef<HTMLDivElement>(null);
  const bufferRef = useRef<SearchResultsBuffer | null>(null);

  // Latest results in a ref so the mount effect can seed the initial document
  // without listing `results` as a dependency (which would churn the whole
  // EditorView on every debounced re-search). Result updates are pushed via
  // setResult in the second effect instead.
  const resultsRef = useRef(results);
  resultsRef.current = results;

  // Mount the CM6 buffer once per store/shell identity; feed later result
  // changes via setResult so the EditorView (and its listeners) survive
  // re-searches.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const buffer = new SearchResultsBuffer(host, resultsRef.current, {
      getSource: (path) => storeApi.getState().getSearchSource(path),
      onSourceEdit: (path, edit) => storeApi.getState().applySearchRowEdit(path, edit),
      onReveal: (path, match) => {
        commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
          kind: "source",
          file: path,
          span: { start: match.start, end: match.end },
        });
      },
    });
    bufferRef.current = buffer;

    // e2e / manual-verification hook, like DocumentSessions' `__brinkView` and
    // CompiledOutputDocument's `__brinkCompiledOutputView`: CM6 renders only the
    // viewport into the DOM, so full-document assertions over the results buffer
    // (file grouping, match counts) need the view's state.
    const w = window as unknown as Record<string, unknown>;
    w.__brinkSearchBufferView = buffer.editorView ?? undefined;

    return () => {
      if (w.__brinkSearchBufferView === buffer.editorView) {
        w.__brinkSearchBufferView = undefined;
      }
      // CM6 teardown: destroy the EditorView + listeners + DOM.
      buffer.destroy();
      bufferRef.current = null;
    };
  }, [storeApi, commands]);

  // Push new results into the live buffer (a fresh search ran) without
  // re-creating the EditorView.
  useEffect(() => {
    bufferRef.current?.setResult(results);
  }, [results]);

  return <div className="search-results-buffer" ref={hostRef} />;
}
