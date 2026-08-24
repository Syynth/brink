/**
 * The perf probe (measure-first ruling, 2026-08-24) is trusted by everything
 * downstream — HUD, run artifacts, `scripts/perf-compare.mjs` — so its
 * contracts are pinned here:
 *
 *  - disabled state records nothing and passes values through untouched
 *    (the production posture);
 *  - spans/marks land in the report with correct aggregation
 *    (count/total/percentiles) and deterministic ordering;
 *  - the ring wraps rather than growing;
 *  - the wasm proxy times method calls without breaking receiver identity
 *    or non-function properties.
 */

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  isPerfEnabled,
  perfMark,
  perfRecord,
  perfReport,
  perfReset,
  perfSpan,
  perfTime,
  setPerfEnabled,
} from "../perf/probe.js";
import { withPerfTiming } from "../perf/wasm-proxy.js";

beforeEach(() => {
  perfReset();
  setPerfEnabled(true);
});

afterEach(() => {
  setPerfEnabled(false);
  perfReset();
});

describe("probe core", () => {
  it("records nothing while disabled and passes values through", () => {
    setPerfEnabled(false);
    expect(isPerfEnabled()).toBe(false);
    const out = perfTime("off.time", () => 42);
    expect(out).toBe(42);
    perfSpan("off.span")();
    perfMark("off.mark");
    perfRecord("off.record", 0, 1);
    const report = perfReport();
    expect(report.windowSize).toBe(0);
    expect(report.aggregates).toEqual([]);
    expect(report.marks).toEqual([]);
  });

  it("aggregates spans by name with count/total/max", () => {
    perfRecord("a", 0, 10);
    perfRecord("a", 20, 30);
    perfRecord("b", 5, 1);
    const report = perfReport();
    expect(report.windowSize).toBe(3);
    const a = report.aggregates.find((s) => s.name === "a");
    expect(a).toBeDefined();
    expect(a?.count).toBe(2);
    expect(a?.totalMs).toBe(40);
    expect(a?.maxMs).toBe(30);
    // Dominant-total-first ordering: "a" (40ms) before "b" (1ms).
    expect(report.aggregates[0]?.name).toBe("a");
  });

  it("carries the numeric annotation into meanMeta", () => {
    perfRecord("meta", 0, 1, 100);
    perfRecord("meta", 0, 1, 300);
    perfRecord("meta", 0, 1);
    const agg = perfReport().aggregates.find((s) => s.name === "meta");
    expect(agg?.meanMeta).toBe(200);
  });

  it("perfSpan measures the open-to-close interval", () => {
    const end = perfSpan("spanny");
    end(7);
    const agg = perfReport().aggregates.find((s) => s.name === "spanny");
    expect(agg?.count).toBe(1);
    expect(agg?.meanMeta).toBe(7);
  });

  it("perfTime returns the callback result and records a span even on throw", () => {
    expect(perfTime("t", () => "ok")).toBe("ok");
    expect(() =>
      perfTime("t.throws", () => {
        throw new Error("boom");
      }),
    ).toThrow("boom");
    const report = perfReport();
    expect(report.aggregates.find((s) => s.name === "t")?.count).toBe(1);
    expect(report.aggregates.find((s) => s.name === "t.throws")?.count).toBe(1);
  });

  it("marks land in the report in time order", () => {
    perfMark("first");
    perfMark("second");
    const marks = perfReport().marks;
    expect(marks.map((m) => m.name)).toEqual(["first", "second"]);
    expect(marks[1].atMs).toBeGreaterThanOrEqual(marks[0].atMs);
  });

  it("the ring wraps: lifetime count grows, window stays bounded", () => {
    for (let i = 0; i < 20000; i++) perfRecord("wrap", i, 1);
    const report = perfReport();
    expect(report.spansRecorded).toBe(20000);
    expect(report.windowSize).toBeLessThanOrEqual(16384);
    expect(report.aggregates.find((s) => s.name === "wrap")?.count).toBe(report.windowSize);
  });

  it("reset drops everything", () => {
    perfRecord("gone", 0, 1);
    perfMark("gone.mark");
    perfReset();
    const report = perfReport();
    expect(report.windowSize).toBe(0);
    expect(report.spansRecorded).toBe(0);
    expect(report.marks).toEqual([]);
  });
});

describe("wasm proxy", () => {
  class FakeSession {
    calls = 0;
    generation = 3;
    query(n: number): number {
      // Receiver identity must be the original instance: internal state
      // mutation through the proxy must land on the real object.
      this.calls++;
      return n * 2;
    }
  }

  it("times method calls under wasm.<method> and preserves behavior", () => {
    const raw = new FakeSession();
    const proxied = withPerfTiming(raw);
    expect(proxied.query(21)).toBe(42);
    expect(raw.calls).toBe(1);
    expect(proxied.generation).toBe(3);
    const agg = perfReport().aggregates.find((s) => s.name === "wasm.query");
    expect(agg?.count).toBe(1);
  });

  it("records nothing through the proxy while disabled", () => {
    setPerfEnabled(false);
    const proxied = withPerfTiming(new FakeSession());
    expect(proxied.query(2)).toBe(4);
    setPerfEnabled(true);
    expect(perfReport().aggregates.find((s) => s.name === "wasm.query")).toBeUndefined();
  });
});
