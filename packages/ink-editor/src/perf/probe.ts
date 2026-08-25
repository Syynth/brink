/**
 * Performance probe — the shared collector behind the desktop perf work
 * (measure-first ruling, docs/decision-log.md 2026-08-24).
 *
 * Design constraints, in order:
 *
 * 1. **Near-zero cost when disabled, bounded cost when enabled.** Every
 *    public entry point bails on one boolean before touching anything else.
 *    Since the prod-perf ruling (docs/decision-log.md 2026-08-25) hosts
 *    enable collection in ALL builds by default — real projects run in
 *    production builds, and dev-only data can't see them — so the enabled
 *    path must stay allocation-free and every retained structure bounded
 *    (the rings below, and the User Timing mirror's periodic self-clear).
 * 2. **No allocation on the record path.** Events land in preallocated
 *    parallel ring buffers; aggregation (grouping, percentiles) happens only
 *    when a report is requested.
 * 3. **DevTools is a first-class consumer.** Every span is also emitted as a
 *    `performance.measure`, so a Chrome/Safari Performance recording shows the
 *    probe's named bars in its Timings track interleaved with the sampled
 *    flame chart. The ring buffer exists so runs can ALSO be exported as JSON
 *    for offline comparison (`scripts/perf-compare.mjs`), independent of a
 *    DevTools session.
 */

/** Ring capacity — at ~6 spans per keystroke this holds several minutes of
 *  active typing; older events fall off rather than grow memory. */
const SPAN_CAPACITY = 16384;
/** Instant marks (startup timeline etc.) are few; a small ring suffices. */
const MARK_CAPACITY = 512;

let enabled = false;

/** One-time feature detection: jsdom (vitest) implements User Timing L2 but
 *  not the L3 options object; a browser that throws once will throw always. */
let measureWithOptions: boolean | null = null;

const spanNames: (string | null)[] = new Array<string | null>(SPAN_CAPACITY).fill(null);
const spanStarts = new Float64Array(SPAN_CAPACITY);
const spanDurs = new Float64Array(SPAN_CAPACITY);
/** Optional numeric annotation per span (doc length, item count, bytes…);
 *  NaN when the site recorded none. */
const spanMetas = new Float64Array(SPAN_CAPACITY);
let spanHead = 0;
let spansRecorded = 0;

const markNames: (string | null)[] = new Array<string | null>(MARK_CAPACITY).fill(null);
const markTimes = new Float64Array(MARK_CAPACITY);
let markHead = 0;
let marksRecorded = 0;

/** Enable/disable collection. Hosts enable at mount by default (prod-perf
 *  ruling 2026-08-25; `MountStudioOptions.perf: false` opts out); tests and
 *  the HUD may toggle it directly. */
export function setPerfEnabled(on: boolean): void {
  enabled = on;
}

/** Names this realm has emitted into the User Timing buffer — kept so the
 *  periodic clear removes ONLY our own entries (an embedding page owns the
 *  rest of its performance timeline). Bounded by the distinct span/mark
 *  names in the codebase, all static literals. */
const emittedTimingNames = new Set<string>();
let timingEntriesSinceClear = 0;
/** Always-on sessions would otherwise grow the User Timing buffer without
 *  bound (one entry per span, forever). Clearing is safe for DevTools: a
 *  recording captures entries at emission; clearing the buffer afterwards
 *  never retracts them from a trace. */
const TIMING_CLEAR_EVERY = 4096;

function noteTimingEntry(name: string): void {
  emittedTimingNames.add(name);
  timingEntriesSinceClear++;
  if (timingEntriesSinceClear < TIMING_CLEAR_EVERY) return;
  timingEntriesSinceClear = 0;
  try {
    for (const n of emittedTimingNames) {
      performance.clearMeasures(n);
      performance.clearMarks(n);
    }
  } catch {
    // A host without clear*: ring collection is unaffected; the mirror
    // grows with the page, which is the pre-clearing status quo.
  }
}

export function isPerfEnabled(): boolean {
  return enabled;
}

function emitMeasure(name: string, start: number, end: number): void {
  if (measureWithOptions === false) return;
  try {
    performance.measure(name, { start, end });
    measureWithOptions = true;
    noteTimingEntry(name);
  } catch {
    // User Timing L3 unavailable (jsdom). Ring-buffer collection still works;
    // only the DevTools Timings-track mirror is lost.
    measureWithOptions = false;
  }
}

function record(name: string, start: number, dur: number, meta: number): void {
  spanNames[spanHead] = name;
  spanStarts[spanHead] = start;
  spanDurs[spanHead] = dur;
  spanMetas[spanHead] = meta;
  spanHead = (spanHead + 1) % SPAN_CAPACITY;
  spansRecorded++;
  emitMeasure(name, start, start + dur);
}

/**
 * Open a span; call the returned function to close it. For code where the
 * start/end aren't a single expression (async seams, CM update plumbing).
 * An optional numeric annotation may be passed at close.
 */
export function perfSpan(name: string): (meta?: number) => void {
  if (!enabled) return noopEnd;
  const start = performance.now();
  return (meta?: number) => {
    record(name, start, performance.now() - start, meta ?? Number.NaN);
  };
}

const noopEnd = (): void => {};

/** Time a synchronous computation. The common form for hot sites. */
export function perfTime<T>(name: string, fn: () => T): T {
  if (!enabled) return fn();
  const start = performance.now();
  try {
    return fn();
  } finally {
    record(name, start, performance.now() - start, Number.NaN);
  }
}

/** Record an already-measured duration (for sites that own their own
 *  clocking — the wasm proxy, observer callbacks re-reporting entries). */
export function perfRecord(name: string, start: number, dur: number, meta?: number): void {
  if (!enabled) return;
  record(name, start, dur, meta ?? Number.NaN);
}

/** Instant mark (startup timeline: project-open, first-compile, …). Also
 *  emitted as a `performance.mark` for DevTools recordings. */
export function perfMark(name: string): void {
  if (!enabled) return;
  const now = performance.now();
  markNames[markHead] = name;
  markTimes[markHead] = now;
  markHead = (markHead + 1) % MARK_CAPACITY;
  marksRecorded++;
  try {
    performance.mark(name);
    noteTimingEntry(name);
  } catch {
    // mark() predates L3 and exists everywhere we run; guard anyway so a
    // headless host without User Timing can't break collection.
  }
}

export interface PerfSpanAggregate {
  name: string;
  count: number;
  totalMs: number;
  meanMs: number;
  p50Ms: number;
  p95Ms: number;
  maxMs: number;
  /** Mean of the numeric annotations, over spans that carried one. */
  meanMeta: number | null;
}

export interface PerfRawSpan {
  name: string;
  startMs: number;
  durMs: number;
  meta: number | null;
}

export interface PerfReport {
  generatedAtMs: number;
  /** Lifetime event count (may exceed the window when the ring wrapped). */
  spansRecorded: number;
  windowSize: number;
  aggregates: PerfSpanAggregate[];
  /** The worst individual spans in the window, largest first. */
  worst: PerfRawSpan[];
  marks: { name: string; atMs: number }[];
}

function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0;
  const idx = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
  return sorted[idx];
}

/** Aggregate the current window. Allocation-heavy by design — called from the
 *  HUD/export path, never from a record site. */
export function perfReport(worstCount = 25): PerfReport {
  const byName = new Map<string, { durs: number[]; starts: number[]; metas: number[] }>();
  const raw: PerfRawSpan[] = [];
  const windowSize = Math.min(spansRecorded, SPAN_CAPACITY);
  for (let i = 0; i < windowSize; i++) {
    const name = spanNames[i];
    if (name === null) continue;
    let bucket = byName.get(name);
    if (!bucket) {
      bucket = { durs: [], starts: [], metas: [] };
      byName.set(name, bucket);
    }
    bucket.durs.push(spanDurs[i]);
    bucket.starts.push(spanStarts[i]);
    if (!Number.isNaN(spanMetas[i])) bucket.metas.push(spanMetas[i]);
    raw.push({
      name,
      startMs: spanStarts[i],
      durMs: spanDurs[i],
      meta: Number.isNaN(spanMetas[i]) ? null : spanMetas[i],
    });
  }

  const aggregates: PerfSpanAggregate[] = [];
  for (const [name, bucket] of byName) {
    const sorted = [...bucket.durs].sort((a, b) => a - b);
    const totalMs = bucket.durs.reduce((a, b) => a + b, 0);
    aggregates.push({
      name,
      count: bucket.durs.length,
      totalMs,
      meanMs: totalMs / bucket.durs.length,
      p50Ms: percentile(sorted, 50),
      p95Ms: percentile(sorted, 95),
      maxMs: sorted[sorted.length - 1] ?? 0,
      meanMeta:
        bucket.metas.length === 0
          ? null
          : bucket.metas.reduce((a, b) => a + b, 0) / bucket.metas.length,
    });
  }
  // Deterministic ordering: dominant cost first, name as tiebreak.
  aggregates.sort((a, b) => b.totalMs - a.totalMs || a.name.localeCompare(b.name));

  raw.sort((a, b) => b.durMs - a.durMs);

  const marks: { name: string; atMs: number }[] = [];
  const markWindow = Math.min(marksRecorded, MARK_CAPACITY);
  for (let i = 0; i < markWindow; i++) {
    const name = markNames[i];
    if (name !== null) marks.push({ name, atMs: markTimes[i] });
  }
  marks.sort((a, b) => a.atMs - b.atMs);

  return {
    generatedAtMs: performance.now(),
    spansRecorded,
    windowSize,
    aggregates,
    worst: raw.slice(0, worstCount),
    marks,
  };
}

/** Drop everything collected so far (scenario boundaries, HUD reset). */
export function perfReset(): void {
  spanNames.fill(null);
  markNames.fill(null);
  spanHead = 0;
  spansRecorded = 0;
  markHead = 0;
  marksRecorded = 0;
}
