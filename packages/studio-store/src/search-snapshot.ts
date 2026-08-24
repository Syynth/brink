/**
 * Search snapshot model (docs/search-results-cards-spec.md, PR B).
 *
 * The frozen-snapshot semantic ruled 2026-08-24: once a search (or Find
 * References) populates the panel, the result set is a snapshot — edits
 * never remove or re-filter rows. Rows whose match no longer satisfies the
 * query are flagged, not dropped; only a new search or an explicit refresh
 * replaces the set.
 *
 * To keep the frozen rows *usable* (reveal, write-through replace) while
 * documents change underneath, every match span is **edit-mapped**: each
 * file in the snapshot remembers the source text its spans were last valid
 * against (`seenSource`), and `remapSnapshot` diffs that against the live
 * source to shift spans through the change. The diff is a single
 * prefix/suffix region — the change-notification seam (the compile path)
 * delivers debounced full-content flushes, not per-keystroke deltas, so one
 * contiguous region is the best available resolution. Spans overlapping the
 * region are conservatively expanded to cover it and flagged `edited`;
 * spans outside it map exactly.
 *
 * Two flags per match, deliberately distinct:
 *
 * - `edited` — an edit has touched this match's span since capture.
 *   Sticky: an undo that restores the original text clears `stale` but
 *   not `edited` (the row was touched; pretending otherwise would make
 *   the flag flap during undo/redo).
 * - `stale` — the live text at the mapped span no longer satisfies the
 *   origin: for a text query, it no longer matches the pattern
 *   (anchored, whole-slice); for references, it differs from the
 *   originally matched text. Recomputed on every remap.
 *
 * Everything here is pure — the slice owns when to capture/remap and where
 * live sources come from.
 */

import {
  buildSearchPattern,
  type ProjectSearchResult,
  type SearchMatch,
  type SearchQueryOptions,
} from "@brink-lang/editor";

// ── Types ───────────────────────────────────────────────────────────

/** What produced the snapshot — frozen so refresh re-runs *this* origin
 *  even if the query input has since changed. */
export type SnapshotOrigin =
  | { kind: "query"; query: string; options: SearchQueryOptions }
  | { kind: "references"; symbol: string };

/** A match with a stable identity and edit-tracking flags. Extends
 *  `SearchMatch` structurally so the existing results buffer (and every
 *  helper typed against `ProjectSearchResult`) consumes a snapshot
 *  unchanged. */
export interface SnapshotMatch extends SearchMatch {
  /** Stable card id: `${path}#${ordinal-at-capture}`. Never changes across
   *  remaps (rows are never dropped or reordered) — collapse state and the
   *  card list key on it. */
  id: string;
  /** An edit has overlapped this span since capture (sticky). */
  edited: boolean;
  /** The live text at the mapped span no longer satisfies the origin. */
  stale: boolean;
}

export interface SnapshotFile {
  path: string;
  matches: SnapshotMatch[];
  /** The source text `matches` spans are currently valid against —
   *  captured at snapshot time, advanced by every remap. */
  seenSource: string;
  /** The file is gone from the session (every match is stale). A later
   *  remap that finds the file again resumes mapping from `seenSource`. */
  deleted: boolean;
}

/** The references declaration anchor, edit-mapped like a match so an
 *  explicit refresh re-resolves from the declaration's *current* position
 *  (the original click offset goes stale). */
export interface SnapshotAnchor {
  file: string;
  start: number;
  end: number;
  /** Text at the anchor at capture (staleness check). */
  text: string;
  /** Source text the span is valid against (the anchor's file may not be
   *  in `files`, so it tracks its own). */
  seenSource: string;
  edited: boolean;
  stale: boolean;
}

/** Structural superset of `ProjectSearchResult` — assignable wherever the
 *  raw result shape is consumed. */
export interface SearchSnapshot {
  files: SnapshotFile[];
  totalMatches: number;
  capped: boolean;
  origin: SnapshotOrigin;
  anchor: SnapshotAnchor | null;
}

// ── Source diffing ──────────────────────────────────────────────────

/** One contiguous changed region: `[start, oldEnd)` in the old text became
 *  `[start, newEnd)` in the new text. */
export interface SourceDiff {
  start: number;
  oldEnd: number;
  newEnd: number;
}

/**
 * Minimal single-region diff via common prefix/suffix. Returns null when
 * the texts are identical. The suffix scan is bounded so the two regions
 * never overlap (classic pitfall: "aba" → "ababa" must not double-count
 * the shared "a").
 */
export function diffSources(oldText: string, newText: string): SourceDiff | null {
  if (oldText === newText) return null;
  const oldLen = oldText.length;
  const newLen = newText.length;
  const maxPrefix = Math.min(oldLen, newLen);
  let prefix = 0;
  while (prefix < maxPrefix && oldText.charCodeAt(prefix) === newText.charCodeAt(prefix)) {
    prefix++;
  }
  const maxSuffix = maxPrefix - prefix;
  let suffix = 0;
  while (
    suffix < maxSuffix &&
    oldText.charCodeAt(oldLen - 1 - suffix) === newText.charCodeAt(newLen - 1 - suffix)
  ) {
    suffix++;
  }
  return { start: prefix, oldEnd: oldLen - suffix, newEnd: newLen - suffix };
}

export interface MappedSpan {
  start: number;
  end: number;
  /** The span overlapped the changed region (could not map exactly). */
  touched: boolean;
}

/**
 * Map a span through a diff. Spans strictly outside the changed region map
 * exactly (before: unchanged; after: shifted by the length delta). A span
 * overlapping the region is expanded to cover it — start associates
 * backward (clamped to the region start), end associates forward (clamped
 * to the region's new end) — and reports `touched`.
 */
export function mapSpan(diff: SourceDiff, start: number, end: number): MappedSpan {
  const delta = diff.newEnd - diff.oldEnd;
  // Entirely before the change (an insertion exactly at `end` leaves the
  // span alone — the match text itself did not move).
  if (end <= diff.start) return { start, end, touched: false };
  // Entirely after the change (an insertion exactly at `start` shifts it).
  if (start >= diff.oldEnd) return { start: start + delta, end: end + delta, touched: false };
  const mappedStart = Math.min(start, diff.start);
  const mappedEnd = end >= diff.oldEnd ? end + delta : diff.newEnd;
  return { start: mappedStart, end: Math.max(mappedEnd, mappedStart), touched: true };
}

// ── Line info ───────────────────────────────────────────────────────

interface LineInfo {
  line: number;
  lineText: string;
  lineStart: number;
  lineEnd: number;
}

/** Recompute a match's line fields from `source` for the span `[start, end)`
 *  — same conventions as the search engine (1-based line of the span start,
 *  full line text without the newline, span clamped to the line). */
export function lineInfoAt(source: string, start: number, end: number): LineInfo {
  const clampedStart = Math.max(0, Math.min(start, source.length));
  let line = 1;
  for (let i = 0; i < clampedStart; i++) {
    if (source.charCodeAt(i) === 10) line++;
  }
  let lineFrom = source.lastIndexOf("\n", clampedStart - 1) + 1;
  if (clampedStart === 0) lineFrom = 0;
  let lineTo = source.indexOf("\n", clampedStart);
  if (lineTo === -1) lineTo = source.length;
  return {
    line,
    lineText: source.slice(lineFrom, lineTo),
    lineStart: clampedStart - lineFrom,
    lineEnd: Math.max(clampedStart, Math.min(end, lineTo)) - lineFrom,
  };
}

// ── Staleness ───────────────────────────────────────────────────────

/** Anchored (whole-slice) pattern for the query-mode staleness check.
 *  Null when the origin's pattern no longer compiles (defensive: it
 *  compiled at capture). */
export function anchoredQueryPattern(
  query: string,
  options: SearchQueryOptions,
): RegExp | null {
  const built = buildSearchPattern(query, options);
  if (!built.ok) return null;
  // Same source, anchored, no global/multiline flags: `test` must consume
  // the entire slice exactly.
  const flags = built.pattern.flags.replace(/[gm]/g, "");
  try {
    return new RegExp(`^(?:${built.pattern.source})$`, flags);
  } catch {
    return null;
  }
}

function isMatchStale(
  origin: SnapshotOrigin,
  anchored: RegExp | null,
  liveText: string,
  originalText: string,
): boolean {
  if (origin.kind === "references") return liveText !== originalText;
  if (anchored === null) return liveText !== originalText;
  return !anchored.test(liveText);
}

// ── Capture ─────────────────────────────────────────────────────────

/**
 * Freeze a raw search result into a snapshot. `getSource` supplies the
 * live text each file's spans are valid against right now (the engine just
 * searched it, so it is also the capture baseline). Files whose source is
 * unreadable are captured as deleted (their matches start stale).
 */
export function captureSnapshot(
  result: ProjectSearchResult,
  origin: SnapshotOrigin,
  getSource: (path: string) => string | null,
  anchorLocation: { file: string; start: number; end: number } | null = null,
): SearchSnapshot {
  const files: SnapshotFile[] = result.files.map((file) => {
    const source = getSource(file.path);
    return {
      path: file.path,
      seenSource: source ?? "",
      deleted: source === null,
      matches: file.matches.map((match, index) => ({
        ...match,
        id: `${file.path}#${index}`,
        edited: false,
        stale: source === null,
      })),
    };
  });

  let anchor: SnapshotAnchor | null = null;
  if (anchorLocation !== null) {
    const source = getSource(anchorLocation.file);
    if (source !== null) {
      anchor = {
        file: anchorLocation.file,
        start: anchorLocation.start,
        end: anchorLocation.end,
        text: source.slice(anchorLocation.start, anchorLocation.end),
        seenSource: source,
        edited: false,
        stale: false,
      };
    }
  }

  return {
    files,
    totalMatches: result.totalMatches,
    capped: result.capped,
    origin,
    anchor,
  };
}

// ── Remap ───────────────────────────────────────────────────────────

/**
 * Map every span in the snapshot through whatever changed since the spans
 * were last valid, and recompute staleness. Pure: returns a new snapshot
 * (or the same object when nothing changed — callers can cheaply skip a
 * store update).
 */
export function remapSnapshot(
  snapshot: SearchSnapshot,
  getSource: (path: string) => string | null,
): SearchSnapshot {
  const anchored =
    snapshot.origin.kind === "query"
      ? anchoredQueryPattern(snapshot.origin.query, snapshot.origin.options)
      : null;

  let changed = false;

  const files = snapshot.files.map((file) => {
    const live = getSource(file.path);
    if (live === null) {
      if (file.deleted) return file;
      changed = true;
      return {
        ...file,
        deleted: true,
        matches: file.matches.map((m) => ({ ...m, edited: true, stale: true })),
      };
    }
    const diff = diffSources(file.seenSource, live);
    if (diff === null && !file.deleted) return file;
    changed = true;
    const matches = file.matches.map((match) => {
      const mapped = diff ? mapSpan(diff, match.start, match.end) : { start: match.start, end: match.end, touched: false };
      const liveText = live.slice(mapped.start, mapped.end);
      const edited = match.edited || mapped.touched || file.deleted;
      const stale = isMatchStale(snapshot.origin, anchored, liveText, match.text);
      return {
        ...match,
        ...lineInfoAt(live, mapped.start, mapped.end),
        start: mapped.start,
        end: mapped.end,
        edited,
        stale,
      };
    });
    return { ...file, matches, seenSource: live, deleted: false };
  });

  let anchor = snapshot.anchor;
  if (anchor !== null) {
    const live = getSource(anchor.file);
    if (live === null) {
      if (!anchor.stale) {
        changed = true;
        anchor = { ...anchor, edited: true, stale: true };
      }
    } else {
      const diff = diffSources(anchor.seenSource, live);
      if (diff !== null || anchor.stale) {
        changed = true;
        const mapped = diff
          ? mapSpan(diff, anchor.start, anchor.end)
          : { start: anchor.start, end: anchor.end, touched: false };
        const liveText = live.slice(mapped.start, mapped.end);
        anchor = {
          ...anchor,
          start: mapped.start,
          end: mapped.end,
          seenSource: live,
          edited: anchor.edited || mapped.touched,
          stale: liveText !== anchor.text,
        };
      }
    }
  }

  if (!changed) return snapshot;
  return { ...snapshot, files, anchor };
}

// ── Context lines ───────────────────────────────────────────────────

/** Card context window: lines shown above/below the match line
 *  (default 1 above, 2 below — ruled with the card design). */
export interface SearchContextLines {
  before: number;
  after: number;
}

export const DEFAULT_SEARCH_CONTEXT_LINES: SearchContextLines = { before: 1, after: 2 };

/** Knob ceiling — a card is a context window, not a file view. */
export const MAX_SEARCH_CONTEXT_LINES = 9;

export function clampContextLines(lines: SearchContextLines): SearchContextLines {
  const clamp = (n: number): number =>
    Math.max(0, Math.min(MAX_SEARCH_CONTEXT_LINES, Math.floor(Number.isFinite(n) ? n : 0)));
  return { before: clamp(lines.before), after: clamp(lines.after) };
}
