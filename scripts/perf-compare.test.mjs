// Tests for scripts/perf-compare.mjs (measure-first ruling, 2026-08-24) —
// the pure halves: aggregate indexing, comparison verdicts, ordering, and
// the rendered table's load-bearing cells. File I/O (loadRun) is exercised
// through a real temp directory.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { aggregateMap, compareRuns, formatComparison, loadRun } from "./perf-compare.mjs";

const agg = (name, totalMs, count = 1, p95Ms = totalMs) => ({
  name,
  count,
  totalMs,
  meanMs: totalMs / count,
  p50Ms: p95Ms,
  p95Ms,
  maxMs: p95Ms,
});

test("aggregateMap indexes by name and tolerates an empty report", () => {
  const map = aggregateMap({ aggregates: [agg("a", 5)] });
  assert.equal(map.get("a").totalMs, 5);
  assert.equal(aggregateMap({}).size, 0);
  assert.equal(aggregateMap(null).size, 0);
});

test("compareRuns verdicts: regression, improvement, noise, added, removed", () => {
  const base = aggregateMap({
    aggregates: [agg("slower", 100), agg("faster", 100), agg("same", 100), agg("gone", 10)],
  });
  const cand = aggregateMap({
    aggregates: [agg("slower", 150), agg("faster", 50), agg("same", 105), agg("fresh", 10)],
  });
  const rows = compareRuns(base, cand);
  const byName = new Map(rows.map((r) => [r.name, r]));
  assert.equal(byName.get("slower").verdict, "▲");
  assert.equal(byName.get("faster").verdict, "▼");
  assert.equal(byName.get("same").verdict, "~");
  assert.equal(byName.get("fresh").verdict, "+");
  assert.equal(byName.get("gone").verdict, "-");
  assert.ok(Math.abs(byName.get("slower").deltaTotal - 0.5) < 1e-9);
});

test("compareRuns orders by dominant total across either run", () => {
  const base = aggregateMap({ aggregates: [agg("small", 1), agg("big", 500)] });
  const cand = aggregateMap({ aggregates: [agg("small", 800), agg("big", 400)] });
  const rows = compareRuns(base, cand);
  // "small" ballooned to 800 in the candidate — it outranks "big" (500).
  assert.equal(rows[0].name, "small");
  assert.equal(rows[1].name, "big");
});

test("compareRuns treats a zero-total baseline as zero delta, not NaN", () => {
  const base = aggregateMap({ aggregates: [agg("zero", 0)] });
  const cand = aggregateMap({ aggregates: [agg("zero", 5)] });
  const rows = compareRuns(base, cand);
  assert.equal(rows[0].deltaTotal, 0);
  assert.equal(rows[0].verdict, "~");
});

test("formatComparison renders every row with its verdict and delta", () => {
  const base = aggregateMap({ aggregates: [agg("wasm.compileProject", 200, 4, 60)] });
  const cand = aggregateMap({ aggregates: [agg("wasm.compileProject", 400, 4, 120)] });
  const text = formatComparison(compareRuns(base, cand), "A", "B");
  assert.match(text, /baseline=A candidate=B/);
  assert.match(text, /▲ wasm\.compileProject/);
  assert.match(text, /\+100%/);
});

test("loadRun reads probe.json and tolerates missing meta/wasm files", () => {
  const dir = mkdtempSync(join(tmpdir(), "perf-compare-test-"));
  mkdirSync(dir, { recursive: true });
  writeFileSync(
    join(dir, "probe.json"),
    JSON.stringify({ aggregates: [agg("x", 3)] }),
  );
  const run = loadRun(dir);
  assert.equal(run.aggregates.get("x").totalMs, 3);
  assert.equal(run.meta, null);
  assert.equal(run.wasm, null);
});
