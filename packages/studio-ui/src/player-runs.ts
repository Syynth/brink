/**
 * Player run folding (#3389, RULED 2026-08-30): delivered transcript rows
 * become RUNS — a cue header once, its dialogue beneath, action/narrative
 * outside — using the project's resolved dialect through the SAME two
 * pieces the editor and an engine use (`DialectParser.parseEmitted` +
 * `runsOf`). No dialect ⇒ every row is a plain line. Studio chrome rows
 * (`marker` choice echoes, `notice`s) never join a run and never parse —
 * which is also what fixes #3390: a story line that merely starts with
 * `> ` is story text, and an echo is an echo because of its `kind`.
 */
import { DialectParser, runsOf, type DialogueDialect, type EmittedSegment } from "@brink-lang/editor";
import type { TranscriptLine } from "@brink/studio-store";

/** One rendered row: the transcript line plus how the dialect read it. */
export interface PlayerRow {
  index: number;
  line: TranscriptLine;
  /** The dialect kind of the line's opening segment (`character`,
   *  `action`, …), `null` for plain text / chrome rows. */
  kind: string | null;
  /** The parsed segments — the cue's own text is `segments[0]` for a
   *  `character` row, so the header and the first spoken line can render
   *  separately. Empty for chrome rows and when no dialect is active. */
  segments: EmittedSegment[];
}

/** One render group: a speaker run (`speaker` set, rows = cue line then
 *  its dialogue) or a standalone row. */
export interface PlayerGroup {
  kind: string | null;
  speaker: string | null;
  rows: PlayerRow[];
}

/** Fold transcript rows into render groups. Pure; memoize on
 *  `(lines, dialect)` at the call site. */
export function foldPlayerRuns(
  lines: readonly TranscriptLine[],
  dialect: DialogueDialect | null,
): PlayerGroup[] {
  if (dialect === null) {
    return lines.map((line, index) => ({
      kind: null,
      speaker: null,
      rows: [{ index, line, kind: null, segments: [] }],
    }));
  }
  const parser = new DialectParser(dialect);
  // Chrome rows are opaque to the dialect: they get NO segments (never
  // parsed), and a choice echo is the turn boundary the reserved
  // "choices" ender keys off.
  const rows: PlayerRow[] = lines.map((line, index) => ({
    index,
    line,
    kind: null,
    segments: line.kind === "line" ? parser.parseEmitted(line.text) : [],
  }));
  for (const row of rows) row.kind = row.segments[0]?.kind ?? null;
  const emitted = rows.map((row, i) => ({
    segments: row.segments,
    boundary: i > 0 && rows[i - 1].line.kind === "marker",
  }));
  const runs = runsOf(emitted, dialect);
  const groups: PlayerGroup[] = [];
  for (const run of runs) {
    // A chrome row is always standalone: split it out of whatever run the
    // fold placed it in (segment-less, it "joined" as plain text). And a
    // choice echo is the READER's turn: it ends the speaker's run in the
    // Player whatever the dialect's run rule says (a preset with no
    // `run_ends_at` kept attributing the narration after a pick to the
    // last speaker — caught live 2026-09-02), so the lines after it are
    // nobody's until the next cue.
    let current: PlayerGroup | null = null;
    let speakerEnded = false;
    for (const idx of run.lines) {
      const row = rows[idx];
      // A knot/stitch change is a scene boundary the dialect cannot see
      // (ruled 2026-09-02: the runtime's `currentPath()`, stamped per row):
      // the speaker's run ends there. Rows without a path (restored
      // history, the first line from the root) never break.
      if (idx > 0 && pathBreak(rows[idx - 1].line, row.line)) {
        if (current) groups.push(current);
        current = null;
        // The run's own first row (a cue in the new knot) keeps its speaker;
        // only rows the OLD run would have carried across lose theirs.
        if (idx !== run.lines[0]) speakerEnded = true;
      }
      if (row.line.kind !== "line") {
        if (current) groups.push(current);
        current = null;
        if (row.line.kind === "marker") speakerEnded = true;
        groups.push({ kind: null, speaker: null, rows: [row] });
        continue;
      }
      if (current === null) {
        current = speakerEnded
          ? { kind: null, speaker: null, rows: [row] }
          : { kind: run.kind, speaker: run.attrs.speaker ?? null, rows: [row] };
      } else {
        current.rows.push(row);
      }
    }
    if (current) groups.push(current);
  }
  // A speaker who keeps talking is one group, however the script cued it
  // (ruled 2026-09-02): per-line cues, or a re-cue with nothing between,
  // print the header once and the lines flow under it. Anything in
  // between — an action, a choice echo, narration — already split the
  // runs, so a genuine re-entrance keeps its own header.
  const merged: PlayerGroup[] = [];
  for (const g of groups) {
    const last = merged[merged.length - 1];
    const lastRow = last?.rows[last.rows.length - 1]?.line;
    const firstRow = g.rows[0]?.line;
    const sameScene =
      lastRow === undefined || firstRow === undefined || !pathBreak(lastRow, firstRow);
    if (last && sameScene && g.speaker !== null && last.speaker === g.speaker && last.kind === g.kind) {
      last.rows.push(...g.rows);
    } else {
      merged.push(g);
    }
  }
  return merged;
}

/** Whether two consecutive rows come from different knots/stitches — only
 *  when BOTH know where they came from. */
function pathBreak(a: TranscriptLine, b: TranscriptLine): boolean {
  return a.path !== undefined && b.path !== undefined && a.path !== b.path;
}

/** Deterministic speaker colour: a stable palette index from the name, so
 *  a cast is distinguishable at a glance without any roster declared
 *  (a declared cast can override this later — Settings, #3392). */
export function speakerPaletteIndex(name: string, size: number): number {
  let h = 2166136261;
  for (let i = 0; i < name.length; i++) {
    h ^= name.charCodeAt(i);
    h = Math.imul(h, 16777619) >>> 0;
  }
  return size > 0 ? h % size : 0;
}
