/**
 * Project-wide search engine (issue #94, spec §4 "Search").
 *
 * Pure string search over file sources — no wasm involvement. All offsets
 * are UTF-16 code units (JavaScript string indices), the same space CM6
 * documents and `editor.reveal` source spans use, so a match span can be
 * dispatched to the navigation protocol verbatim.
 *
 * The non-regex path escapes the query and still runs through one RegExp
 * loop, so case/whole-word/regex compose uniformly. Results are capped
 * (unbounded-growth guard): a pathological query over a large project
 * stops at SEARCH_RESULT_CAP matches and reports `capped`.
 */

export interface SearchQueryOptions {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
}

export const DEFAULT_SEARCH_OPTIONS: SearchQueryOptions = {
  caseSensitive: false,
  wholeWord: false,
  regex: false,
};

/** Hard cap on total matches per search (unbounded-growth guard). */
export const SEARCH_RESULT_CAP = 1000;

/** Escape a literal string for embedding in a RegExp source. */
export function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export type SearchPatternResult =
  | { ok: true; pattern: RegExp }
  | { ok: false; error: string };

/**
 * Compile query + options into the search RegExp. Non-regex queries are
 * escaped (always valid); regex queries are validated *before* the
 * whole-word wrapping so the inline error points at the user's input, not
 * at our decoration. Flags: global + multiline (^/$ match per line, the
 * VS Code convention), case-insensitive unless requested.
 */
export function buildSearchPattern(
  query: string,
  options: SearchQueryOptions,
): SearchPatternResult {
  let source: string;
  if (options.regex) {
    try {
      new RegExp(query);
    } catch (error) {
      return { ok: false, error: `Invalid regex: ${(error as Error).message}` };
    }
    source = query;
  } else {
    source = escapeRegExp(query);
  }
  if (options.wholeWord) source = `\\b(?:${source})\\b`;
  const flags = options.caseSensitive ? "gm" : "gim";
  try {
    return { ok: true, pattern: new RegExp(source, flags) };
  } catch (error) {
    // Group-wrapping a valid pattern cannot fail, but keep the seam total.
    return { ok: false, error: `Invalid regex: ${(error as Error).message}` };
  }
}

export interface SearchMatch {
  /** Match span in UTF-16 offsets into the file source (editor.reveal space). */
  start: number;
  end: number;
  /** 1-based line number of the match start. */
  line: number;
  /** Full text of the line containing the match start (no newline). */
  lineText: string;
  /** Match span within `lineText` (end clamped to the line). */
  lineStart: number;
  lineEnd: number;
  /** The matched text (replacement preview + stale-result guard). */
  text: string;
}

export interface FileSearchResult {
  path: string;
  matches: SearchMatch[];
}

export interface ProjectSearchResult {
  /** Files with at least one match, in input (sorted-path) order. */
  files: FileSearchResult[];
  totalMatches: number;
  /** True when the search stopped at the result cap. */
  capped: boolean;
}

/**
 * Convert symbol-reference locations into the search-result shape, so Find
 * References renders through the same results buffer as text search
 * (context-menu spec ruling: the Search panel is the references surface).
 * Locations are grouped by file in sorted-path order; unreadable files are
 * skipped.
 */
export function locationsToSearchResult(
  locations: readonly { file: string; start: number; end: number }[],
  getSource: (path: string) => string | null,
): ProjectSearchResult {
  const byFile = new Map<string, { start: number; end: number }[]>();
  for (const loc of locations) {
    let list = byFile.get(loc.file);
    if (!list) {
      list = [];
      byFile.set(loc.file, list);
    }
    list.push({ start: loc.start, end: loc.end });
  }
  const files: FileSearchResult[] = [];
  let total = 0;
  for (const path of [...byFile.keys()].sort()) {
    const source = getSource(path);
    if (source === null) continue;
    const spans = byFile.get(path) ?? [];
    spans.sort((a, b) => a.start - b.start);
    const matches: SearchMatch[] = [];
    for (const span of spans) {
      if (span.start > source.length) continue;
      const lineStartIdx = source.lastIndexOf("\n", Math.max(0, span.start - 1)) + 1;
      let lineEndIdx = source.indexOf("\n", span.start);
      if (lineEndIdx < 0) lineEndIdx = source.length;
      const lineText = source.slice(lineStartIdx, lineEndIdx);
      const line = source.slice(0, lineStartIdx).split("\n").length;
      matches.push({
        start: span.start,
        end: span.end,
        line,
        lineText,
        lineStart: span.start - lineStartIdx,
        lineEnd: Math.min(span.end, lineEndIdx) - lineStartIdx,
        text: source.slice(span.start, span.end),
      });
    }
    if (matches.length > 0) {
      files.push({ path, matches });
      total += matches.length;
    }
  }
  return { files, totalMatches: total, capped: false };
}

/**
 * Run `pattern` (a `g`-flagged RegExp) over every source, grouping matches
 * by file. Stops at `cap` total matches. Zero-length matches (e.g. `a*`)
 * advance by one code unit so the loop always terminates.
 */
export function searchSources(
  sources: ReadonlyArray<{ path: string; source: string }>,
  pattern: RegExp,
  cap: number = SEARCH_RESULT_CAP,
): ProjectSearchResult {
  const files: FileSearchResult[] = [];
  let total = 0;
  let capped = false;

  for (const { path, source } of sources) {
    if (capped) break;
    pattern.lastIndex = 0;
    let matches: SearchMatch[] | null = null;
    let lineStarts: number[] | null = null;

    let m: RegExpExecArray | null;
    while ((m = pattern.exec(source)) !== null) {
      if (total >= cap) {
        capped = true;
        break;
      }
      lineStarts ??= lineStartsOf(source);
      const start = m.index;
      const end = start + m[0].length;
      const lineIndex = lineIndexAt(lineStarts, start);
      const lineStart = lineStarts[lineIndex];
      const lineEnd =
        lineIndex + 1 < lineStarts.length
          ? lineStarts[lineIndex + 1] - 1
          : source.length;
      matches ??= [];
      matches.push({
        start,
        end,
        line: lineIndex + 1,
        lineText: source.slice(lineStart, lineEnd),
        lineStart: start - lineStart,
        lineEnd: Math.min(end, lineEnd) - lineStart,
        text: m[0],
      });
      total++;
      if (m[0].length === 0) pattern.lastIndex++;
    }

    if (matches !== null) files.push({ path, matches });
  }

  return { files, totalMatches: total, capped };
}

/**
 * The text a match is replaced with. Literal searches use the replacement
 * verbatim; regex searches re-run the (non-global) pattern over the matched
 * text so capture-group references ($1, $&, …) expand.
 */
export function replacementTextFor(
  match: SearchMatch,
  pattern: RegExp,
  replacement: string,
  isRegex: boolean,
): string {
  if (!isRegex) return replacement;
  const anchored = new RegExp(pattern.source, pattern.flags.replace("g", ""));
  return match.text.replace(anchored, replacement);
}

export interface ReplacementEdit {
  start: number;
  end: number;
  text: string;
}

/** Apply non-overlapping span edits to a source (descending-start order). */
export function applyReplacements(
  source: string,
  edits: ReadonlyArray<ReplacementEdit>,
): string {
  const sorted = [...edits].sort((a, b) => b.start - a.start);
  let result = source;
  for (const edit of sorted) {
    result = result.slice(0, edit.start) + edit.text + result.slice(edit.end);
  }
  return result;
}

// ── Result-row display segments ──────────────────────────────────────

/** Leading-context budget before the highlighted match in a result row. */
export const SEARCH_CONTEXT_BEFORE = 32;

export interface MatchLineSegments {
  before: string;
  matchText: string;
  after: string;
}

/**
 * Split a match's line into before/match/after display segments: leading
 * whitespace stripped, long leading context elided to the last
 * SEARCH_CONTEXT_BEFORE units (the match itself must stay visible in a
 * narrow dock).
 */
export function matchLineSegments(match: SearchMatch): MatchLineSegments {
  let before = match.lineText.slice(0, match.lineStart).trimStart();
  if (before.length > SEARCH_CONTEXT_BEFORE) {
    before = `…${before.slice(before.length - SEARCH_CONTEXT_BEFORE)}`;
  }
  return {
    before,
    matchText: match.lineText.slice(match.lineStart, match.lineEnd),
    after: match.lineText.slice(match.lineEnd),
  };
}

// ── Line table helpers ───────────────────────────────────────────────

/** Offsets of every line start (always includes 0). */
function lineStartsOf(source: string): number[] {
  const starts = [0];
  for (let i = 0; i < source.length; i++) {
    if (source.charCodeAt(i) === 10 /* \n */) starts.push(i + 1);
  }
  return starts;
}

/** Index of the last line start ≤ offset (binary search). */
function lineIndexAt(starts: number[], offset: number): number {
  let lo = 0;
  let hi = starts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= offset) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}
