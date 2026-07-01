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
  EditorSelection,
  EditorState,
  type Extension,
  type Transaction,
} from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
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
   *
   * Edits are *committed*, not fired per keystroke: a match row is written
   * back once the user pauses typing ({@link SearchResultsBufferOptions.commitDelayMs})
   * or moves focus away. This keeps a multi-character replacement (e.g.
   * `figure` → `shadow`) a single, coherent source write + compile rather than
   * one write/compile/re-search per keystroke.
   */
  onSourceEdit(path: string, edit: ReplacementEdit): void;
  /** Reveal a match in the normal editor (row activation — dbl-click / Enter). */
  onReveal?(path: string, match: SearchMatch): void;
  /**
   * Idle delay (ms) after the last keystroke before a pending match-row edit is
   * committed to the source. Defaults to {@link DEFAULT_COMMIT_DELAY_MS}. Any
   * pending edit is also flushed immediately on blur and on `destroy()`. Pass 0
   * to commit synchronously (used by tests that dispatch a single atomic edit).
   */
  commitDelayMs?: number;
}

/** Default idle window before a match-row edit is written back to the source. */
export const DEFAULT_COMMIT_DELAY_MS = 350;

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
  private readonly commitDelayMs: number;
  private view: EditorView | null = null;
  private model: ResultsBufferModel = { text: "", rows: [] };
  /** Set of buffer line numbers with an edit awaiting commit (debounced). */
  private pendingLines = new Set<number>();
  private commitTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    host: HTMLElement,
    result: ProjectSearchResult,
    options: SearchResultsBufferOptions,
  ) {
    this.host = host;
    this.options = options;
    this.commitDelayMs = options.commitDelayMs ?? DEFAULT_COMMIT_DELAY_MS;
    this.model = buildResultsRows(result);
    this.mount();
  }

  /**
   * Replace the displayed results (a new search ran). Re-derives the buffer
   * text + row table and resets the document. Keeps the same EditorView so
   * scroll wiring / listeners are not re-created churnily.
   *
   * The reset preserves the user's caret: without this, a full-document swap
   * collapses the selection to offset 0 (into a read-only header), which — when
   * the host re-runs the search after each committed edit — would yank the
   * cursor out of the row the moment an edit lands. When the new text is
   * identical to what is already displayed (the common case: an edit that
   * doesn't change which lines match), the document is left untouched so there
   * is no churn at all.
   */
  setResult(result: ProjectSearchResult): void {
    this.model = buildResultsRows(result);
    const view = this.view;
    if (!view) return;

    // No content change ⇒ nothing to do; leave the doc (and caret) untouched.
    if (view.state.doc.toString() === this.model.text) return;

    // Preserve the caret across the swap by clamping the current head/anchor
    // into the new document length (the row it sat on may have moved or gone).
    const prev = view.state.selection.main;
    const nextLen = this.model.text.length;
    const anchor = Math.min(prev.anchor, nextLen);
    const head = Math.min(prev.head, nextLen);

    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: this.model.text },
      selection: EditorSelection.range(anchor, head),
      // A programmatic reset is not a user edit — mark it so the filter lets
      // it through without trying to map it back to the source.
      annotations: RESET.of(true),
    });
  }

  /**
   * Tear down: flush any pending edit, destroy the EditorView (removes its DOM
   * + the transaction filter and update listener registered as extensions) and
   * clear the host. Mirrors the ConflictView teardown contract — leaks are
   * bugs.
   */
  destroy(): void {
    this.flushPendingCommit();
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

    // On a doc change to a match line, mark that line dirty and (re)arm the
    // commit timer. Edits are *not* written back per keystroke — a
    // multi-character replacement is one coherent source write, committed once
    // the user pauses (see onUserEdit / flushPendingCommit).
    const listener = EditorView.updateListener.of((update) => {
      if (!update.docChanged) return;
      for (const tr of update.transactions) {
        if (tr.annotation(RESET)) continue;
        this.onUserEdit(tr);
      }
    });

    // Commit any pending edit the moment focus leaves the buffer, so a row the
    // user finished editing is written back even before the idle timer fires.
    const blur = EditorView.domEventHandlers({
      blur: () => {
        this.flushPendingCommit();
        return false;
      },
    });

    this.view = new EditorView({
      state: EditorState.create({
        doc: this.model.text,
        extensions: [...baseExtensions(), filter, listener, blur, this.revealKeymap()],
      }),
      parent: this.host,
    });
  }

  /**
   * Permit a transaction only if every changed range lies within the
   * *editable* (source-text) portion of a single match line, and no inserted
   * text introduces a line break. Header, blank, and prefix (line-number)
   * regions are read-only; a change touching them, spanning a line boundary,
   * or *inserting* a newline (Enter, a multi-line paste) is rejected (returns
   * the empty transaction spec, i.e. no change).
   *
   * The inserted-newline guard is essential: without it a pure insertion whose
   * `fromA === toA` sits on a single old-doc line, so the old-doc-range check
   * alone would accept text containing `\n`. That would split one buffer row
   * into two and permanently desync the row table (`this.model.rows`) from the
   * document — every later row would look up the wrong file/line. A match row
   * is a single source line by construction; it must stay one line.
   */
  private filterTransaction(tr: Transaction): Transaction | readonly Transaction[] {
    if (!tr.docChanged) return tr;
    if (tr.annotation(RESET)) return tr;

    let ok = true;
    tr.changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
      if (!ok) return;
      // Reject any inserted line break — a match row must stay a single line.
      if (inserted.lines > 1) {
        ok = false;
        return;
      }
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

  /**
   * Record which match rows a user edit touched, then (re)arm the idle timer.
   * The actual source write-back happens in {@link flushPendingCommit}, once
   * the user pauses ({@link commitDelayMs}) or focus leaves the buffer — never
   * per keystroke. `commitDelayMs === 0` commits synchronously (test path).
   */
  private onUserEdit(tr: Transaction): void {
    const doc = tr.state.doc;
    tr.changes.iterChangedRanges((_fromA, _toA, fromB) => {
      this.pendingLines.add(doc.lineAt(fromB).number);
    });
    if (this.pendingLines.size === 0) return;

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

  /**
   * Write every pending match-row edit back to its source (deduped by line),
   * mapping the *current* buffer line through {@link mapRowEditToSource} so the
   * committed text reflects the full edit, not an intermediate keystroke.
   * Clears the timer + pending set. Idempotent — a no-op when nothing pends.
   */
  private flushPendingCommit(): void {
    if (this.commitTimer !== null) {
      clearTimeout(this.commitTimer);
      this.commitTimer = null;
    }
    if (this.pendingLines.size === 0) return;
    const view = this.view;
    if (!view) {
      this.pendingLines.clear();
      return;
    }
    const doc = view.state.doc;
    const lines = [...this.pendingLines].sort((a, b) => a - b);
    this.pendingLines.clear();
    for (const lineNumber of lines) {
      if (lineNumber < 1 || lineNumber > doc.lines) continue;
      const row = this.model.rows[lineNumber - 1];
      if (!row || row.kind !== "match") continue;
      const bufferLine = doc.line(lineNumber).text;
      const source = this.options.getSource(row.path);
      if (source === null) continue; // file gone — stale, skip
      const edit = mapRowEditToSource(row, bufferLine, source);
      if (edit !== null) this.options.onSourceEdit(row.path, edit);
    }
  }

  /**
   * Reveal the match on the caret's line in the normal editor. Wired to both a
   * double-click and an Enter / Mod-Enter keymap so the surface is
   * keyboard-reachable (a keyboard-only user can focus a row and press Enter);
   * returns false on non-match lines so the key falls through to the default.
   */
  private revealKeymap(): Extension {
    const reveal = (view: EditorView): boolean => {
      const pos = view.state.selection.main.head;
      const lineNumber = view.state.doc.lineAt(pos).number;
      const row = this.model.rows[lineNumber - 1];
      if (row && row.kind === "match") {
        this.options.onReveal?.(row.path, row.match);
        return true;
      }
      return false;
    };
    return [
      EditorView.domEventHandlers({
        dblclick: (_event, view) => reveal(view),
      }),
      keymap.of([
        { key: "Enter", run: reveal },
        { key: "Mod-Enter", run: reveal },
      ]),
    ];
  }
}
