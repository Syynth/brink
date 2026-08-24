#!/usr/bin/env node
// Offline comparison of recorded perf runs (measure-first ruling,
// docs/decision-log.md 2026-08-24).
//
// A "run" is a directory written by the perf scenario runner
// (`pnpm --filter @brink-lang/studio test:perf`) or by hand from the HUD's
// Copy JSON: at minimum `probe.json` (the @brink-lang/editor probe report),
// optionally `wasm-counters.json` and `meta.json`. Runs live under
// `perf-runs/` (gitignored).
//
// Usage:
//   node scripts/perf-compare.mjs <baseline-run-dir> <candidate-run-dir> [...more candidates]
//   pnpm perf:compare -- perf-runs/A perf-runs/B
//
// Prints, per span name, count / total / p50 / p95 / max for every run and
// the candidate-vs-baseline delta on total. Regressions (total grew beyond
// the threshold) are marked `▲`, improvements `▼`. Purely informational —
// exit code is 0 unless inputs are unreadable; judging a delta is the
// reader's job, per the ruling (no fixes, no green/red gates on timings).

import { readFileSync } from "node:fs";
import { join } from "node:path";
import process from "node:process";

/** Relative change below this is reported as "~" (noise). */
const NOISE_THRESHOLD = 0.1;

/** Read one run directory into { name, aggregates: Map, meta, wasm }. */
export function loadRun(dir) {
  const probe = JSON.parse(readFileSync(join(dir, "probe.json"), "utf8"));
  let meta = null;
  let wasm = null;
  try {
    meta = JSON.parse(readFileSync(join(dir, "meta.json"), "utf8"));
  } catch {
    // meta is optional (hand-collected runs)
  }
  try {
    wasm = JSON.parse(readFileSync(join(dir, "wasm-counters.json"), "utf8"));
  } catch {
    // wasm counters are optional
  }
  return { dir, aggregates: aggregateMap(probe), meta, wasm };
}

/** Index a probe report's aggregates by span name. */
export function aggregateMap(probe) {
  const map = new Map();
  for (const a of probe?.aggregates ?? []) map.set(a.name, a);
  return map;
}

/**
 * Compare one candidate against the baseline. Returns rows sorted by the
 * larger of the two totals (dominant cost first), each row:
 * { name, base, cand, deltaTotal, verdict } where verdict is "▲" (slower
 * beyond the noise threshold), "▼" (faster), "~" (within noise), "+"
 * (new in candidate), "-" (gone in candidate).
 */
export function compareRuns(baseMap, candMap, noise = NOISE_THRESHOLD) {
  const names = new Set([...baseMap.keys(), ...candMap.keys()]);
  const rows = [];
  for (const name of names) {
    const base = baseMap.get(name) ?? null;
    const cand = candMap.get(name) ?? null;
    let verdict;
    let deltaTotal = null;
    if (base === null) verdict = "+";
    else if (cand === null) verdict = "-";
    else {
      deltaTotal = base.totalMs === 0 ? 0 : (cand.totalMs - base.totalMs) / base.totalMs;
      verdict = deltaTotal > noise ? "▲" : deltaTotal < -noise ? "▼" : "~";
    }
    rows.push({ name, base, cand, deltaTotal, verdict });
  }
  rows.sort(
    (a, b) =>
      Math.max(b.base?.totalMs ?? 0, b.cand?.totalMs ?? 0) -
      Math.max(a.base?.totalMs ?? 0, a.cand?.totalMs ?? 0),
  );
  return rows;
}

function fmt(ms) {
  if (ms === null || ms === undefined) return "—";
  return ms >= 100 ? ms.toFixed(0) : ms >= 10 ? ms.toFixed(1) : ms.toFixed(2);
}

function fmtDelta(delta) {
  if (delta === null) return "—";
  const pct = (delta * 100).toFixed(0);
  return delta >= 0 ? `+${pct}%` : `${pct}%`;
}

/** Render comparison rows as an aligned text table. */
export function formatComparison(rows, baseLabel, candLabel) {
  const lines = [];
  lines.push(
    `perf-compare | baseline=${baseLabel} candidate=${candLabel}`,
    "perf-compare | span | base count/total/p95 | cand count/total/p95 | Δtotal",
  );
  for (const r of rows) {
    const base = r.base
      ? `${r.base.count}× ${fmt(r.base.totalMs)}ms p95=${fmt(r.base.p95Ms)}`
      : "—";
    const cand = r.cand
      ? `${r.cand.count}× ${fmt(r.cand.totalMs)}ms p95=${fmt(r.cand.p95Ms)}`
      : "—";
    lines.push(
      `perf-compare | ${r.verdict} ${r.name.padEnd(36)} | ${base.padEnd(28)} | ${cand.padEnd(28)} | ${fmtDelta(r.deltaTotal)}`,
    );
  }
  return lines.join("\n");
}

function main(argv) {
  const dirs = argv.slice(2);
  if (dirs.length < 2) {
    console.error(
      "usage: node scripts/perf-compare.mjs <baseline-run-dir> <candidate-run-dir> [...more]",
    );
    return 1;
  }
  let base;
  try {
    base = loadRun(dirs[0]);
  } catch (err) {
    console.error(`cannot read baseline run ${dirs[0]}: ${err.message}`);
    return 1;
  }
  for (const dir of dirs.slice(1)) {
    let cand;
    try {
      cand = loadRun(dir);
    } catch (err) {
      console.error(`cannot read candidate run ${dir}: ${err.message}`);
      return 1;
    }
    const rows = compareRuns(base.aggregates, cand.aggregates);
    console.log(formatComparison(rows, base.dir, cand.dir));
    console.log("");
  }
  return 0;
}

// Import-safe: running as a CLI executes main; importing (tests) does not.
if (import.meta.url === `file://${process.argv[1]}`) {
  process.exit(main(process.argv));
}
