import { memo, useCallback, useMemo, useState } from "react";
import { useShell } from "@brink/studio-shell";
import { useStudioStore } from "./StoreContext.js";
import { DebugValueView } from "./DebugValueView.js";
import type { DebugFrame } from "@brink/wasm-types";

/**
 * State View — a read-only runtime debugger.
 *
 * Session-bound (spec §7.6): renders the structured `DebugState` snapshot
 * (status, current location, globals, call stack, visit counts, pending
 * choices, rng) from the story session, refreshed whenever the story
 * advances. Values that changed since the previous step are highlighted
 * (diffed against `prevDebugState`). With no session, a placeholder with a
 * `story.start` affordance is shown instead.
 */
/**
 * One frame's named locals (#3140).
 *
 * Rendered INSIDE the frame rather than as its own top-level section: a
 * local is scoped to the call frame that owns it, and two frames can hold
 * different `gold`s at once. A flat "Locals" section would have to invent a
 * disambiguation the data does not have.
 *
 * The `locals` field is deliberately tri-state on the wire, and all three
 * states say something different to an author:
 *
 * - `undefined` — the program carries no `DebugInfo` (release-exported, or
 *   compiled before D6), or the frame has no live position to resolve a
 *   scope from. NOT the same as "no locals", so it says so.
 * - `[]` — debug info is present and this frame genuinely declares none.
 * - non-empty — the names, with live values.
 *
 * Collapsing the first two into a blank space is the one thing this must
 * not do: an author looking at an empty panel would read "this function has
 * no locals" when the truth is "this build cannot tell you".
 */
function FrameLocals({ frame }: { frame: DebugFrame }) {
  if (frame.locals === undefined) {
    // Only worth saying when the frame plausibly HAS locals — an exhausted
    // or external frame carries no position by construction, and noting
    // that on every one of them is noise.
    return frame.temps > 0 ? (
      <p className="sv-locals-none sv-dim">no debug info for this frame</p>
    ) : null;
  }
  // #3395: compiler-minted temps (the lift-order hoist's `$liftN`) are
  // hidden, per `docs/debugger-spec.md` §3 — same rule as the Debugger
  // panel's FrameLocals.
  const locals = frame.locals.filter((l) => !l.synthetic);
  if (locals.length === 0) return null;

  return (
    <table className="sv-locals">
      <tbody>
        {locals.map((l) => (
          // Keyed by SLOT, not name: the slot is the frame-unique
          // identity, and a future codegen that reused a name across
          // sibling scopes would otherwise collide.
          <tr key={l.slot}>
            <td className="sv-key">{l.name}</td>
            <td className="sv-val sv-mono">
              <DebugValueView value={l.value} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function StateViewInner() {
  const sessionStatus = useStudioStore((s) => s.sessionStatus);
  const debugState = useStudioStore((s) => s.debugState);
  const prevDebugState = useStudioStore((s) => s.prevDebugState);
  const { commands } = useShell();
  const [visitFilter, setVisitFilter] = useState("");

  const handleStart = useCallback(() => {
    commands.dispatch("story.start");
  }, [commands]);

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

  // All hooks above; conditional renders below (Rules of Hooks).
  if (sessionStatus === "none") {
    return (
      <div className="state-view">
        <div className="session-placeholder">
          <p className="session-placeholder-title">No story session</p>
          <p className="session-placeholder-hint">
            Start the story to inspect its variables, current location, call
            stack, and visit counts here.
          </p>
          <button className="session-placeholder-start" onClick={handleStart}>
            Start story
          </button>
        </div>
      </div>
    );
  }

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
                <div className="sv-frame-head">
                  <span className={"sv-badge sv-frame-" + f.kind}>{f.kind}</span>
                  <span className="sv-path">{f.location ?? "—"}</span>
                  {/* The bare count stays: it is the one number that is
                      right even with no debug info, and D7 added `locals`
                      ALONGSIDE it rather than replacing it. */}
                  {f.temps > 0 && (
                    <span className="sv-dim">
                      {f.temps} temp{f.temps > 1 ? "s" : ""}
                    </span>
                  )}
                </div>
                <FrameLocals frame={f} />
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
