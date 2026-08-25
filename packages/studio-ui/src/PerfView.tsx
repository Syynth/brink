/**
 * Performance HUD (measure-first ruling 2026-08-24; prod-perf ruling
 * 2026-08-25) — tool window over the `@brink-lang/editor` perf probe.
 *
 * Registered by `mountStudio` in ALL builds (unless the embedder passes
 * `perf: false`): real projects are opened in production builds, and a
 * dev-only HUD can't see them. Renders three planes:
 *
 *  - the MAIN realm's probe (aggregates, worst events, startup marks) plus
 *    the main session's wasm-internal counters — the keystroke/paint side;
 *  - the WORKER realm's bundle (its probe's `wasm.<method>` spans + its
 *    wasm counters), fetched through the host-level `hostPerfReport`
 *    query — since W5 this is where the analysis cost actually lives.
 *
 * The view polls on a 1s interval while mounted. Aggregation is
 * allocation-heavy by design (probe contract) and the worker fetch is a
 * background query, so the poll only costs anything while the window is
 * actually open — a closed HUD costs nothing. "Copy JSON" exports every
 * plane in one payload for offline comparison (`scripts/perf-compare.mjs`).
 * The payload is structurally content-free: span/counter names are static
 * code literals and every value is a number — nothing from the author's
 * project (paths, source, symbol names) can appear in it.
 */

import { memo, useCallback, useEffect, useRef, useState } from "react";
import {
  isPerfEnabled,
  perfReport,
  perfReset,
  setPerfEnabled,
  type HostPerfBundle,
  type PerfReport,
} from "@brink-lang/editor";

const POLL_MS = 1000;

/** Shape of the wasm-internal counters (`getPerfCounters`), structural so
 *  this package needs no `@brink-lang/web` dependency. */
export type WasmCounterMap = Record<string, { count: number; totalMs: number; maxMs: number }>;

/** The main-realm host's bridge to the session planes the probe module
 *  itself can't reach. Every member is optional-by-construction at the
 *  call sites: an injected mock session simply yields nulls. */
export interface PerfViewBridge {
  /** Main session's wasm-internal counters (null when unavailable). */
  wasmCounters?: () => WasmCounterMap | null;
  /** Worker-realm bundle; resolves null on the in-process road. */
  fetchWorker?: () => Promise<HostPerfBundle | null>;
  /** Mirror a Pause/Record toggle into the worker realm. */
  setWorkerEnabled?: (on: boolean) => void;
  /** Mirror a Reset into the worker realm + wasm counters. */
  resetWorker?: () => void;
}

function fmt(ms: number): string {
  return ms >= 100 ? ms.toFixed(0) : ms >= 10 ? ms.toFixed(1) : ms.toFixed(2);
}

function CounterTable({ counters }: { counters: WasmCounterMap }) {
  const rows = Object.entries(counters).sort(
    (a, b) => b[1].totalMs - a[1].totalMs || a[0].localeCompare(b[0]),
  );
  if (rows.length === 0) return null;
  return (
    <table className="perf-table">
      <thead>
        <tr>
          <th className="perf-name">counter</th>
          <th>count</th>
          <th>total</th>
          <th>mean</th>
          <th>max</th>
        </tr>
      </thead>
      <tbody>
        {rows.map(([name, c]) => (
          <tr key={name}>
            <td className="perf-name">{name}</td>
            <td>{c.count}</td>
            <td>{fmt(c.totalMs)}</td>
            <td>{fmt(c.count > 0 ? c.totalMs / c.count : 0)}</td>
            <td>{fmt(c.maxMs)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function AggregateTable({ report }: { report: PerfReport }) {
  return (
    <table className="perf-table">
      <thead>
        <tr>
          <th className="perf-name">span</th>
          <th>count</th>
          <th>total</th>
          <th>p50</th>
          <th>p95</th>
          <th>max</th>
        </tr>
      </thead>
      <tbody>
        {report.aggregates.map((a) => (
          <tr key={a.name}>
            <td className="perf-name">{a.name}</td>
            <td>{a.count}</td>
            <td>{fmt(a.totalMs)}</td>
            <td>{fmt(a.p50Ms)}</td>
            <td>{fmt(a.p95Ms)}</td>
            <td>{fmt(a.maxMs)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function PerfViewInner({ bridge }: { bridge?: PerfViewBridge }) {
  const [report, setReport] = useState<PerfReport | null>(null);
  const [mainCounters, setMainCounters] = useState<WasmCounterMap | null>(null);
  const [worker, setWorker] = useState<HostPerfBundle | null>(null);
  const [enabled, setEnabled] = useState(isPerfEnabled);
  const [copied, setCopied] = useState(false);
  const bridgeRef = useRef(bridge);
  bridgeRef.current = bridge;

  useEffect(() => {
    let alive = true;
    let fetching = false;
    const refresh = () => {
      setReport(perfReport());
      setMainCounters(bridgeRef.current?.wasmCounters?.() ?? null);
      const fetchWorker = bridgeRef.current?.fetchWorker;
      if (fetchWorker && !fetching) {
        fetching = true;
        fetchWorker()
          .then((bundle) => {
            if (alive) setWorker(bundle);
          })
          .catch(() => {
            if (alive) setWorker(null);
          })
          .finally(() => {
            fetching = false;
          });
      }
    };
    refresh();
    const timer = setInterval(refresh, POLL_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  const toggle = useCallback(() => {
    const next = !isPerfEnabled();
    setPerfEnabled(next);
    bridgeRef.current?.setWorkerEnabled?.(next);
    setEnabled(next);
  }, []);

  const reset = useCallback(() => {
    perfReset();
    bridgeRef.current?.resetWorker?.();
    setReport(perfReport());
    setWorker(null);
  }, []);

  const copyJson = useCallback(() => {
    const payload = {
      main: {
        probe: perfReport(),
        wasmCounters: bridgeRef.current?.wasmCounters?.() ?? null,
      },
      worker,
    };
    const json = JSON.stringify(payload, null, 2);
    void navigator.clipboard.writeText(json).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, [worker]);

  return (
    <div className="perf-view">
      <div className="perf-toolbar">
        <button type="button" className="perf-button" onClick={toggle}>
          {enabled ? "Pause" : "Record"}
        </button>
        <button type="button" className="perf-button" onClick={reset}>
          Reset
        </button>
        <button type="button" className="perf-button" onClick={copyJson}>
          {copied ? "Copied!" : "Copy JSON"}
        </button>
        {report && (
          <span className="perf-summary">
            {report.spansRecorded} events{report.spansRecorded > report.windowSize ? " (wrapped)" : ""}
            {worker ? " · worker live" : ""}
          </span>
        )}
      </div>
      {!report || report.aggregates.length === 0 ? (
        <p className="perf-empty">
          {enabled
            ? "No events yet — type or scroll in an editor."
            : "Recording paused. Press Record to collect."}
        </p>
      ) : (
        <div className="perf-scroll">
          <AggregateTable report={report} />
          {mainCounters && Object.keys(mainCounters).length > 0 && (
            <>
              <div className="perf-section">Wasm counters (main)</div>
              <CounterTable counters={mainCounters} />
            </>
          )}
          {worker && (
            <>
              <div className="perf-section">Worker: wasm boundary</div>
              {worker.probe.aggregates.length > 0 ? (
                <AggregateTable report={worker.probe} />
              ) : (
                <p className="perf-empty">No worker spans yet.</p>
              )}
              {worker.wasmCounters && Object.keys(worker.wasmCounters).length > 0 && (
                <>
                  <div className="perf-section">Worker: wasm counters</div>
                  <CounterTable counters={worker.wasmCounters} />
                </>
              )}
            </>
          )}
          {report.worst.length > 0 && (
            <>
              <div className="perf-section">Worst events</div>
              <table className="perf-table">
                <tbody>
                  {report.worst.slice(0, 10).map((w, i) => (
                    <tr key={i}>
                      <td className="perf-name">{w.name}</td>
                      <td>{fmt(w.durMs)} ms</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
          {report.marks.length > 0 && (
            <>
              <div className="perf-section">Marks</div>
              <table className="perf-table">
                <tbody>
                  {report.marks.map((m, i) => (
                    <tr key={i}>
                      <td className="perf-name">{m.name}</td>
                      <td>{fmt(m.atMs)} ms</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </>
          )}
        </div>
      )}
    </div>
  );
}

export const PerfView = memo(PerfViewInner);
