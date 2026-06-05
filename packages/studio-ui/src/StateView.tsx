import { memo, useMemo, useState } from "react";
import { useStudioStore } from "./StoreContext.js";

/**
 * State View — a read-only runtime debugger.
 *
 * Renders the structured `DebugState` snapshot (status, current location,
 * globals, call stack, visit counts, pending choices, rng), refreshed whenever
 * the story advances. Values that changed since the previous step are
 * highlighted (diffed against `prevDebugState`).
 */
function StateViewInner() {
  const debugState = useStudioStore((s) => s.debugState);
  const prevDebugState = useStudioStore((s) => s.prevDebugState);
  const [visitFilter, setVisitFilter] = useState("");

  // Globals whose value changed since the previous snapshot.
  const changedGlobals = useMemo(() => {
    const changed = new Set<string>();
    if (!debugState) return changed;
    const prev = new Map(prevDebugState?.globals.map((g) => [g.name, g.value]));
    for (const g of debugState.globals) {
      if (prevDebugState && prev.get(g.name) !== g.value) changed.add(g.name);
    }
    return changed;
  }, [debugState, prevDebugState]);

  if (!debugState) {
    return (
      <div className="state-view">
        <div className="state-view-empty">
          <p className="state-view-empty-title">No running story</p>
          <p className="state-view-empty-hint">
            Run a story in the player to inspect its variables, current location,
            call stack, and visit counts here.
          </p>
        </div>
      </div>
    );
  }

  const locationChanged =
    !!prevDebugState && prevDebugState.current_location !== debugState.current_location;

  const visits = debugState.visit_counts.filter((v) =>
    visitFilter ? v.path.toLowerCase().includes(visitFilter.toLowerCase()) : true,
  );

  return (
    <div className="state-view">
      {/* Status + location */}
      <Section title="Location">
        <div className="sv-row">
          <span className="sv-key">status</span>
          <span className={"sv-badge sv-status-" + debugState.status}>{debugState.status}</span>
        </div>
        <div className="sv-row">
          <span className="sv-key">at</span>
          <span className={"sv-val sv-path" + (locationChanged ? " sv-changed" : "")}>
            {debugState.current_location ?? "—"}
          </span>
        </div>
        <div className="sv-row">
          <span className="sv-key">turn</span>
          <span className="sv-val">{debugState.turn_index}</span>
        </div>
      </Section>

      {/* Globals */}
      <Section title={`Globals (${debugState.globals.length})`}>
        {debugState.globals.length === 0 ? (
          <p className="sv-empty">none</p>
        ) : (
          <table className="sv-table">
            <tbody>
              {debugState.globals.map((g) => (
                <tr key={g.name} className={changedGlobals.has(g.name) ? "sv-changed-row" : ""}>
                  <td className="sv-key">{g.name}</td>
                  <td className="sv-val sv-mono">{g.value}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>

      {/* Call stack */}
      <Section title={`Call stack (${debugState.call_stack.length})`}>
        {debugState.call_stack.length === 0 ? (
          <p className="sv-empty">empty</p>
        ) : (
          <ol className="sv-stack">
            {debugState.call_stack.map((f, i) => (
              <li key={i} className="sv-frame">
                <span className={"sv-badge sv-frame-" + f.kind}>{f.kind}</span>
                <span className="sv-path">{f.location ?? "—"}</span>
                {f.temps > 0 && <span className="sv-dim">{f.temps} temp{f.temps > 1 ? "s" : ""}</span>}
              </li>
            ))}
          </ol>
        )}
      </Section>

      {/* Pending choices */}
      {debugState.pending_choices.length > 0 && (
        <Section title={`Pending choices (${debugState.pending_choices.length})`}>
          <ol className="sv-choices">
            {debugState.pending_choices.map((c, i) => (
              <li key={i} className="sv-row">
                <span className="sv-val">{c.text}</span>
                {c.target && <span className="sv-path sv-dim">→ {c.target}</span>}
              </li>
            ))}
          </ol>
        </Section>
      )}

      {/* Visit counts */}
      <Section title={`Visit counts (${debugState.visit_counts.length})`}>
        {debugState.visit_counts.length > 8 && (
          <input
            className="sv-filter"
            type="text"
            placeholder="filter knots…"
            value={visitFilter}
            onChange={(e) => setVisitFilter(e.target.value)}
          />
        )}
        {visits.length === 0 ? (
          <p className="sv-empty">{visitFilter ? "no matches" : "none yet"}</p>
        ) : (
          <table className="sv-table">
            <tbody>
              {visits.map((v) => (
                <tr key={v.path}>
                  <td className="sv-key sv-path">{v.path}</td>
                  <td className="sv-val sv-num">{v.count}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>

      {/* RNG */}
      <Section title="RNG">
        <div className="sv-row">
          <span className="sv-key">seed</span>
          <span className="sv-val sv-num">{debugState.rng.seed}</span>
        </div>
        <div className="sv-row">
          <span className="sv-key">last</span>
          <span className="sv-val sv-num">{debugState.rng.previous}</span>
        </div>
      </Section>
    </div>
  );
}

// ── Collapsible section ─────────────────────────────────────────────

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <div className="sv-section">
      <button
        type="button"
        className={"sv-section-header" + (open ? " open" : "")}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="sv-chevron">{open ? "▾" : "▸"}</span>
        {title}
      </button>
      {open && <div className="sv-section-body">{children}</div>}
    </div>
  );
}

export const StateView = memo(StateViewInner);
