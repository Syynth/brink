/**
 * search-results-buffer — the editor-owned *editable* search results buffer
 * (issue #322, Track V, design D — the Zed-style ask).
 *
 * The read-only search dock (SearchView) renders a collapsible tree of match
 * rows. This module is the alternative surface the design locks in: a single
 * synthetic CodeMirror document whose lines mirror the cross-file results —
 * a header line per file (`path` + match count) followed by one line per
 * match (`  <line>: <full source line>`). Editing a *match* row rewrites the
 * corresponding source line in the underlying document, routed back through
 * the shared apply-edits seam (`ProjectSession.applyEdit`).
 *
 * The pure half lives here so it is unit-testable without CodeMirror:
 *
 *  - {@link buildResultsRows} maps a {@link ProjectSearchResult} to the
 *    synthetic buffer text plus a per-line row table (header | match | blank).
 *  - {@link mapRowEditToSource} maps an edited match line back to a
 *    {@link ReplacementEdit} over the source file — *safely*: a stale row
 *    (the live source no longer matches what the search recorded) or a
 *    multi-line match (the recorded line is only a prefix of the real span)
 *    is skipped, never mis-applied.
 *
 * {@link SearchResultsBuffer} is the CM6 half: a self-contained EditorView
 * that renders the buffer, keeps non-match lines read-only, and — on a
 * committed edit to a match line — calls back with the source edit. It
 * follows the CodeActionsMenu / ConflictView teardown contract: `destroy()`
 * removes the view and every listener, so nothing leaks when the dock closes.
 */

import {
  Annotation,
  EditorState,
  type Extension,
  type Transaction,
} from "@codemirror/state";
import { EditorView, lineNumbers } from "@codemirror/view";
import { brinkTheme } from "./theme.js";
import type {
  ProjectSearchResult,
  ReplacementEdit,
  SearchMatch,
} from "./project-search.js";

/** Prefix rendered before a match line's source text (line-number gutter). */
const MATCH_INDENT = "  ";

/** A row in the synthetic results buffer, keyed by 0-based buffer line. */
export type ResultRow =
  | { kind: "header"; path: string; matchCount: number }
  | {
      kind: "match";
      path: string;
      match: SearchMatch;
      /** Column (UTF-16) in the buffer line where the source line text starts. */
      sourceCol: number;
    }
  | { kind: "blank" };

export interface ResultsBufferModel {
  /** The synthetic document text (newline-joined rows). */
  text: string;
  /** One entry per line of `text`, same index as the 0-based line number. */
  rows: ResultRow[];
}

/**
 * Build the synthetic buffer text + row table from a search result. Layout,
 * one file after another (blank line between files, in the result's file
 * order — already sorted upstream for determinism):
 *
 *     path/to/a.ink (2)
 *       12: the full source line of the match
 *       40: another matching line
 *     <blank>
 *     path/to/b.ink (1)
 *       3: yet another line
 *
 * The match line renders the *full* source line (`match.lineText`), so an
 * edit maps cleanly back to the source line's span. `sourceCol` is where that
 * source text starts within the buffer line (after the indent + `N: `).
 */
export function buildResultsRows(result: ProjectSearchResult): ResultsBufferModel {
  const lines: string[] = [];
  const rows: ResultRow[] = [];

  result.files.forEach((file, fileIndex) => {
    if (fileIndex > 0) {
      lines.push("");
      rows.push({ kind: "blank" });
    }
    lines.push(`${file.path} (${file.matches.length})`);
    rows.push({ kind: "header", path: file.path, matchCount: file.matches.length });

    for (const match of file.matches) {
      const prefix = `${MATCH_INDENT}${match.line}: `;
      lines.push(`${prefix}${match.lineText}`);
      rows.push({
        kind: "match",
        path: file.path,
        match,
        sourceCol: prefix.length,
      });
    }
  });

  return { text: lines.join("\n"), rows };
}

/**
 * Given a match row and the buffer line's *current* text, plus the live
 * source of the underlying file, compute the source edit — or `null` if the
 * edit is unsafe to apply.
 *
 * Safety rules (stale / multi-line guard, per the locked design):
 *
 *  - The recorded match must be single-line: a multi-line match (e.g. a regex
 *    spanning a newline — `match.text` contains a `\n`) has a `lineText` that
 *    is only the first physical line of the real span, so rewriting that line
 *    would corrupt the file — skip it.
 *  - The live source line the match sits on must still equal the recorded
 *    `lineText` at `[lineStartOffset .. lineStartOffset+len]`. If the file
 *    changed underneath (a stale row), skip.
 *  - The edited buffer text must actually differ from the recorded line
 *    (no-op edits produce no source edit).
 *
 * When safe, returns a {@link ReplacementEdit} that replaces the whole source
 * line span with the edited text.
 */
export function mapRowEditToSource(
  row: Extract<ResultRow, { kind: "match" }>,
  editedBufferLine: string,
  source: string,
): ReplacementEdit | null {
  const { match, sourceCol } = row;

  // The edited *source* portion is the buffer line minus the fixed prefix.
  // A shrunk line (the user deleted into the prefix) is treated as an empty
  // source line rather than reaching negative — clamp to the source column.
  const newSourceText =
    editedBufferLine.length >= sourceCol ? editedBufferLine.slice(sourceCol) : "";

  // Multi-line match guard: a match whose text spans a newline (e.g. a regex
  // crossing lines) has a `lineText` that is only the *first* physical line —
  // rewriting that line would corrupt the rest of the span. Skip it. (Single
  // physical line ⇒ the recorded span sits entirely within `lineText`.)
  if (match.text.includes("\n")) return null;

  // The full source-line span. `match.start - match.lineStart` walks back to
  // the line start; `+ lineText.length` is the line end (newline excluded).
  const lineStartOffset = match.start - match.lineStart;
  const lineEndOffset = lineStartOffset + match.lineText.length;

  // Bounds + stale guard: the live source must still carry the recorded line.
  if (lineStartOffset < 0 || lineEndOffset > source.length) return null;
  if (source.slice(lineStartOffset, lineEndOffset) !== match.lineText) return null;

  // No-op edit: buffer line unchanged from what we rendered.
  if (newSourceText === match.lineText) return null;

  return { start: lineStartOffset, end: lineEndOffset, text: newSourceText };
}

// ── CM6 surface ──────────────────────────────────────────────────────

/** Callbacks the results buffer routes edits + navigation through. */
export interface SearchResultsBufferOptions {
  /**
   * Read the current live source of a file (post-edit truth). Returns null if
   * the file is gone — the row is then treated as stale and skipped.
   */
  getSource(path: string): string | null;
  /**
   * Apply a mapped source edit. The host wires this to the shared apply-edits
   * seam (`ProjectSession.applyEdit` + invalidate + compile) and typically
   * re-runs the search, which feeds a fresh {@link ResultsBufferModel} back in
   * via {@link SearchResultsBuffer.setResult}.
   */
  onSourceEdit(path: string, edit: ReplacementEdit): void;
  /** Reveal a match in the normal editor (row activation — click / Enter). */
  onReveal?(path: string, match: SearchMatch): void;
}

function baseExtensions(): Extension[] {
  return [lineNumbers(), brinkTheme, EditorView.lineWrapping];
}

/** Annotation marking a programmatic (setResult) document reset — such a
 *  transaction bypasses the read-only filter and is not mapped to a source
 *  edit. */
const RESET = Annotation.define<boolean>();

export class SearchResultsBuffer {
  private readonly host: HTMLElement;
  private readonly options: SearchResultsBufferOptions;
  private view: EditorView | null = null;
  private model: ResultsBufferModel = { text: "", rows: [] };

  constructor(
    host: HTMLElement,
    result: ProjectSearchResult,
    options: SearchResultsBufferOptions,
  ) {
    this.host = host;
    this.options = options;
    this.model = buildResultsRows(result);
    this.mount();
  }

  /**
   * Replace the displayed results (a new search ran). Re-derives the buffer
   * text + row table and resets the document. Keeps the same EditorView so
   * scroll wiring / listeners are not re-created churnily.
   */
  setResult(result: ProjectSearchResult): void {
    this.model = buildResultsRows(result);
    if (this.view) {
      this.view.dispatch({
        changes: { from: 0, to: this.view.state.doc.length, insert: this.model.text },
        // A programmatic reset is not a user edit — mark it so the filter lets
        // it through without trying to map it back to the source.
        annotations: RESET.of(true),
      });
    }
  }

  /**
   * Tear down: destroy the EditorView (removes its DOM + the transaction
   * filter and update listener registered as extensions) and clear the host.
   * Mirrors the ConflictView teardown contract — leaks are bugs.
   */
  destroy(): void {
    if (this.view) {
      this.view.destroy();
      this.view = null;
    }
    this.host.replaceChildren();
  }

  // ── Internal ─────────────────────────────────────────────────────

  private mount(): void {
    // A transaction filter enforces the editable contract: user edits are
    // permitted only when they stay within a single match line's source
    // portion; edits touching headers / blanks / line-number prefixes are
    // dropped. Programmatic resets (setResult) bypass the filter.
    const filter = EditorState.transactionFilter.of((tr) => this.filterTransaction(tr));

    // On a committed doc change to a match line, map it back to a source edit.
    const listener = EditorView.updateListener.of((update) => {
      if (!update.docChanged) return;
      for (const tr of update.transactions) {
        if (tr.annotation(RESET)) continue;
        this.onUserEdit(tr);
      }
    });

    this.view = new EditorView({
      state: EditorState.create({
        doc: this.model.text,
        extensions: [...baseExtensions(), filter, listener, this.revealKeymap()],
      }),
      parent: this.host,
    });
  }

  /**
   * Permit a transaction only if every changed range lies within the
   * *editable* (source-text) portion of a single match line. Header, blank,
   * and prefix (line-number) regions are read-only; a change touching them,
   * or spanning a line boundary, is rejected (returns the empty transaction
   * spec, i.e. no change).
   */
  private filterTransaction(tr: Transaction): Transaction | readonly Transaction[] {
    if (!tr.docChanged) return tr;
    if (tr.annotation(RESET)) return tr;

    let ok = true;
    tr.changes.iterChangedRanges((fromA, toA) => {
      if (!ok) return;
      if (!this.rangeIsEditable(fromA, toA)) ok = false;
    });
    return ok ? tr : [];
  }

  /** True iff [from,to] (old-doc offsets) sits inside one match line's source
   *  region (at or after `sourceCol`, not crossing the line boundary). */
  private rangeIsEditable(from: number, to: number): boolean {
    const doc = this.view?.state.doc;
    if (!doc) return false;
    const lineFrom = doc.lineAt(from);
    const lineTo = doc.lineAt(to);
    if (lineFrom.number !== lineTo.number) return false; // no multi-line edits
    const row = this.model.rows[lineFrom.number - 1];
    if (!row || row.kind !== "match") return false;
    // Editable region starts at the source column (protect the `N: ` prefix).
    return from - lineFrom.from >= row.sourceCol;
  }

  /** Map a committed user edit to a source edit and forward it. */
  private onUserEdit(tr: Transaction): void {
    const doc = tr.state.doc;
    const touched = new Set<number>();
    tr.changes.iterChangedRanges((_fromA, _toA, fromB) => {
      touched.add(doc.lineAt(fromB).number);
    });
    for (const lineNumber of touched) {
      const row = this.model.rows[lineNumber - 1];
      if (!row || row.kind !== "match") continue;
      const bufferLine = doc.line(lineNumber).text;
      const source = this.options.getSource(row.path);
      if (source === null) continue; // file gone — stale, skip
      const edit = mapRowEditToSource(row, bufferLine, source);
      if (edit !== null) this.options.onSourceEdit(row.path, edit);
    }
  }

  /** Enter / Mod-Enter on a match line reveals it in the normal editor. */
  private revealKeymap(): Extension {
    return EditorView.domEventHandlers({
      dblclick: (_event, view) => {
        const pos = view.state.selection.main.head;
        const lineNumber = view.state.doc.lineAt(pos).number;
        const row = this.model.rows[lineNumber - 1];
        if (row && row.kind === "match") {
          this.options.onReveal?.(row.path, row.match);
          return true;
        }
        return false;
      },
    });
  }
}
