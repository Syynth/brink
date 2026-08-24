/**
 * Performance HUD (measure-first ruling, docs/decision-log.md 2026-08-24) —
 * dev-only tool window over the `@brink-lang/editor` perf probe.
 *
 * Registered by `mountStudio` ONLY in dev builds; nothing here ships in a
 * production bundle's registry. Renders the probe's aggregate table (worst
 * total first), the worst individual events, and the startup marks — and
 * exports the full report as JSON for the run-artifact workflow
 * (`perf-runs/`, `scripts/perf-compare.mjs`).
 *
 * The view polls `perfReport()` on a 1s interval while mounted. Aggregation
 * is allocation-heavy by design (probe contract), so the poll only runs when
 * the window is actually open — a closed HUD costs nothing.
 */

import { memo, useCallback, useEffect, useState } from "react";
import {
  isPerfEnabled,
  perfReport,
  perfReset,
  setPerfEnabled,
  type PerfReport,
} from "@brink-lang/editor";

const POLL_MS = 1000;

function fmt(ms: number): string {
  return ms >= 100 ? ms.toFixed(0) : ms >= 10 ? ms.toFixed(1) : ms.toFixed(2);
}

function PerfViewInner() {
  const [report, setReport] = useState<PerfReport | null>(null);
  const [enabled, setEnabled] = useState(isPerfEnabled);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const refresh = () => setReport(perfReport());
    refresh();
    const timer = setInterval(refresh, POLL_MS);
    return () => clearInterval(timer);
  }, []);

  const toggle = useCallback(() => {
    const next = !isPerfEnabled();
    setPerfEnabled(next);
    setEnabled(next);
  }, []);

  const reset = useCallback(() => {
    perfReset();
    setReport(perfReport());
  }, []);

  const copyJson = useCallback(() => {
    const json = JSON.stringify(perfReport(), null, 2);
    void navigator.clipboard.writeText(json).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, []);

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
