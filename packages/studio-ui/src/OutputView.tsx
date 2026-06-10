/**
 * Output — append-only compile/runtime log (docs/studio-shell-spec.md §4).
 *
 * Renders the output slice's capped entry log (compile success/failure from
 * the compile callbacks, story runtime errors from the session slice — see
 * studio-store/slices/output.ts). Auto-scrolls to the newest entry; "Clear"
 * empties the log.
 */

import { memo, useEffect, useRef } from "react";
import { useStudioStore } from "./StoreContext.js";

/** "HH:MM:SS" for an epoch-ms timestamp (local time, locale-independent). */
export function formatOutputTimestamp(timestamp: number): string {
  const d = new Date(timestamp);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function OutputViewInner() {
  const entries = useStudioStore((s) => s.outputEntries);
  const clearOutput = useStudioStore((s) => s.clearOutput);
  const logRef = useRef<HTMLDivElement | null>(null);

  // Follow the tail: jump to the bottom whenever entries arrive.
  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [entries.length]);

  return (
    <div className="output-view">
      <div className="output-toolbar">
        <button
          type="button"
          className="output-clear"
          onClick={clearOutput}
          disabled={entries.length === 0}
        >
          Clear
        </button>
      </div>
      <div className="output-log" ref={logRef}>
        {entries.length === 0 ? (
          <p className="output-empty">No output yet</p>
        ) : (
          entries.map((entry, i) => (
            <div key={i} className="output-entry">
              <span className="output-time">{formatOutputTimestamp(entry.timestamp)}</span>
              <span className={`output-source output-source-${entry.source}`}>
                {entry.source}
              </span>
              <span className="output-message">{entry.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

export const OutputView = memo(OutputViewInner);
