/**
 * Debugger panel (W8/#3301) — the StateView replacement (RULED
 * 2026-08-29: redesign, not extension), in StateView's strip slot.
 *
 * Sections top-to-bottom (spec §4): Flows (RULED: the open-flows list
 * lives HERE, not the status bar — `SessionPicker` retires; selection
 * scopes everything below) · Frames (interactive: click selects, scopes
 * the Variables section's locals, draws the editor's accent frame band,
 * and reveals the frame's position) · Variables (selected frame's
 * locals, then globals with the step diff highlight — editing is W16) ·
 * Breakpoints (the source anchors: checkbox enable/disable,
 * click-to-reveal, remove; header disable-all/clear-all) · Story
 * (collapsed: the old StateView's inspection content, demoted).
 *
 * Watch (W17), value editing (W16) and data breakpoints (W18) land in
 * their own work items. Placeholders keep StateView's honesty: no
 * session → start affordance; no debug info → names the App setting.
 */
import { memo, useCallback, useMemo, useRef, useState } from "react";
import {
  EDITOR_REVEAL_COMMAND_ID,
  encodeProgramAddress,
  useShell,
} from "@brink/studio-shell";
import { DEFAULT_SESSION_ID, isDebugSessionProvider } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";
import { DebugValueView } from "./DebugValueView.js";
import type { DebugFrame, DebugValue } from "@brink/wasm-types";

/** Seed text for editing a scalar local — the display form the parse
 * road accepts back (strings quoted, matching the panel's rendering). */
function scalarSeed(value: DebugValue): string | null {
  switch (value.type) {
    case "int":
    case "float":
      return String(value.value);
    case "bool":
      return value.value ? "true" : "false";
    case "string":
      return `"${value.value}"`;
    default:
      return null; // lists/structs/etc. stay read-only in v1 (RULED)
  }
}

/** A global's display string, when it reads as an editable scalar. */
function globalIsScalar(display: string): boolean {
  return /^-?\d+(\.\d+)?$|^true$|^false$|^".*"$/s.test(display);
}

/**
 * Live value editing (W16/#3309, RULED — paused-only, scalars only):
 * click → inline mono input; Enter commits, Esc cancels; a refused edit
 * (parse/type failure — the wasm boundary checks against the CURRENT
 * type) red-shakes and keeps the input; a committed one closes and
 * flashes the value.
 */
function EditableScalar({
  display,
  disabled,
  disabledReason,
  commit,
}: {
  display: string;
  disabled: boolean;
  disabledReason: string;
  commit: (input: string) => boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [shake, setShake] = useState(false);
  const [flash, setFlash] = useState(false);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // ZERO layout impact while editing (maintainer feedback — "literally
  // everything moves"): the display span STAYS IN FLOW (hidden) so the
  // table's auto-layout never sees the input at all — an in-flow input's
  // intrinsic width widened the value column and shifted every row's
  // columns. The input overlays the cell absolutely (the cell is the
  // positioned ancestor), glyphs anchored where the span's were.
  return (
    <>
      <span
        className={
          "dp-editable" +
          (disabled ? " dp-editable-off" : "") +
          (flash ? " dp-edited-flash" : "") +
          (editing ? " dp-editing" : "")
        }
        title={disabled ? disabledReason : "Click to edit — Enter commits, Esc cancels"}
        onClick={() => {
          if (!disabled) setEditing(true);
        }}
      >
        {display}
      </span>
      {editing && (
        <input
          autoFocus
          className={"dp-value-input sv-mono" + (shake ? " dp-shake" : "")}
          defaultValue={display}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              if (commit(e.currentTarget.value)) {
                setEditing(false);
                setFlash(true);
                if (flashTimer.current) clearTimeout(flashTimer.current);
                flashTimer.current = setTimeout(() => setFlash(false), 900);
              } else {
                setShake(true);
              }
            } else if (e.key === "Escape") {
              setEditing(false);
            }
          }}
          onAnimationEnd={() => setShake(false)}
          onBlur={() => setEditing(false)}
        />
      )}
    </>
  );
}

/** One frame's named locals — unchanged from StateView (#3140); see the
 * original tri-state doc: `undefined` = no debug info (says so), `[]` =
 * genuinely none (renders nothing), non-empty = names + live values.
 * W16: scalar values are click-to-edit while paused — except at a choice
 * stop, where choosing restores the choice's captured thread and would
 * silently overwrite the edit (measured; see the provider's doc). */
function FrameLocals({
  frame,
  frameIdx,
  editable,
  editDisabledReason,
}: {
  frame: DebugFrame;
  frameIdx: number;
  editable: boolean;
  editDisabledReason: string;
}) {
  const debugEditTemp = useStudioStore((s) => s.debugEditTemp);
  if (frame.locals === undefined) {
    return frame.temps > 0 ? (
      <p className="sv-locals-none sv-dim">no debug info for this frame</p>
    ) : null;
  }
  if (frame.locals.length === 0) return null;

  return (
    <table className="sv-locals">
      <tbody>
        {frame.locals.map((l) => {
          const seed = scalarSeed(l.value);
          return (
            <tr key={l.slot}>
              <td className="sv-key">{l.name}</td>
              <td className="sv-val sv-mono">
                {seed === null ? (
                  <DebugValueView value={l.value} />
                ) : (
                  <EditableScalar
                    display={seed}
                    disabled={!editable}
                    disabledReason={editDisabledReason}
                    commit={(input) => debugEditTemp(frameIdx, l.slot, input)}
                  />
                )}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** The transport mirror in the tool window's header-actions slot —
 * stepping works with the Player hidden (spec §4 item 1). */
function DebuggerActionsInner() {
  const { commands } = useShell();
  const debugCapable = useStudioStore((s) => s.debugCapable);
  const paused = useStudioStore((s) => s.sessionPaused);
  const status = useStudioStore((s) => s.sessionStatus);
  if (!debugCapable) return null;

  return (
    <span className="dp-actions">
      {paused ? (
        <button
          type="button"
          className="dp-action"
          title="Continue — run to the next line of content and resume play"
          aria-label="Continue"
          onClick={() => commands.dispatch("debug.continue")}
        >
          {"▶"}
        </button>
      ) : (
        <button
          type="button"
          className="dp-action"
          title="Pause"
          aria-label="Pause"
          disabled={status !== "running" && status !== "awaiting-choice"}
          onClick={() => commands.dispatch("debug.pause")}
        >
          {"⏸"}
        </button>
      )}
      <button
        type="button"
        className="dp-action"
        title="Step over"
        aria-label="Step over"
        disabled={!paused}
        onClick={() => commands.dispatch("debug.stepOver")}
      >
        {"⤴"}
      </button>
      <button
        type="button"
        className="dp-action"
        title="Step into"
        aria-label="Step into"
        disabled={!paused}
        onClick={() => commands.dispatch("debug.stepInto")}
      >
        {"⤵"}
      </button>
    </span>
  );
}
export const DebuggerActions = memo(DebuggerActionsInner);

function DebuggerPanelInner() {
  const sessionStatus = useStudioStore((s) => s.sessionStatus);
  const debugState = useStudioStore((s) => s.debugState);
  const prevDebugState = useStudioStore((s) => s.prevDebugState);
  const paused = useStudioStore((s) => s.sessionPaused);
  const sessions = useStudioStore((s) => s.sessions);
  const activeSessionId = useStudioStore((s) => s.activeSessionId);
  const setActiveSession = useStudioStore((s) => s.setActiveSession);
  const openSession = useStudioStore((s) => s.openSession);
  const openFlow = useStudioStore((s) => s.openFlow);
  const closeSession = useStudioStore((s) => s.closeSession);
  const selectedFrameIdx = useStudioStore((s) => s.selectedFrameIdx);
  const debugEditGlobal = useStudioStore((s) => s.debugEditGlobal);
  const selectFrame = useStudioStore((s) => s.selectFrame);
  const sourceBreakpoints = useStudioStore((s) => s.sourceBreakpoints);
  const breakpointSetEnabled = useStudioStore((s) => s.breakpointSetEnabled);
  const breakpointRemove = useStudioStore((s) => s.breakpointRemove);
  const breakpointsDisableAll = useStudioStore((s) => s.breakpointsDisableAll);
  const breakpointsClearAll = useStudioStore((s) => s.breakpointsClearAll);
  const debugInfoEnabled = useStudioStore((s) => s.debugInfoEnabled);
  // Frame rows show `file:line` when the position resolves (render-time
  // read through the provider — small list, degraded handled by null).
  // NOTE: select the PROVIDER, derive the function in render — a selector
  // returning a fresh `.bind()` identity per call trips React's
  // getSnapshot caching check into an infinite loop (found live).
  const provider = useStudioStore((s) => s._provider);
  const resolveDebugLine =
    provider !== null && isDebugSessionProvider(provider)
      ? (c: number, o: number) => {
          try {
            return provider.resolveDebugLine(c, o);
          } catch {
            return null;
          }
        }
      : null;
  const resolveSourceBytes = useStudioStore((s) => s._resolveSourceBytes);
  const { commands } = useShell();
  const [visitFilter, setVisitFilter] = useState("");

  const handleStart = useCallback(() => {
    commands.dispatch("story.start");
  }, [commands]);

  const revealPosition = useCallback(
    (pos: { container_idx: number; offset: number }) => {
      // Prefer the debug-line road — the EXACT line the row's label shows
      // (the program resolver resolves container→symbol→header, which can
      // land a knot away from the statement). Fall back to program-kind.
      const line = resolveDebugLine?.(pos.container_idx, pos.offset) ?? null;
      if (line !== null && resolveSourceBytes) {
        const point = resolveSourceBytes(
          line.file,
          line.range_start,
          line.range_start + line.range_len,
        );
        if (point !== null) {
          commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
            kind: "source",
            file: line.file,
            span: { start: point.start, end: point.end },
          });
          return;
        }
      }
      commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
        kind: "program",
        address: encodeProgramAddress(pos.container_idx, pos.offset),
      });
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps -- resolveDebugLine
    // is derived per-render from the provider; the provider identity is the
    // real dependency.
    [commands, provider, resolveSourceBytes],
  );

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
      <div className="state-view debugger-panel">
        <div className="session-placeholder">
          <p className="session-placeholder-title">No story session</p>
          <p className="session-placeholder-hint">
            Start the story to debug it here — flows, call frames,
            variables, and breakpoints.
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
      <div className="state-view debugger-panel">
        <div className="state-view-empty">
          <p className="state-view-empty-title">No running story</p>
          <p className="state-view-empty-hint">
            {debugInfoEnabled
              ? "Run a story in the player to debug it here."
              : 'Debug info is off (Settings → Debugging, "Emit debug info in studio compiles") — turn it on to debug here.'}
          </p>
        </div>
      </div>
    );
  }

  const frames = debugState.call_stack;
  const effectiveFrameIdx = selectedFrameIdx ?? 0;
  const selectedFrame = frames[effectiveFrameIdx];
  const visits = debugState.visit_counts.filter((v) =>
    visitFilter ? v.path.toLowerCase().includes(visitFilter.toLowerCase()) : true,
  );

  return (
    <div className="state-view debugger-panel">
      {/* Status line mirroring the Player chip (spec §4 item 1). */}
      <div className="dp-status">
        <span className={"sv-badge sv-status-" + debugState.status}>
          {paused ? "paused" : debugState.status}
        </span>
        <span className="sv-path">{debugState.current_location ?? "—"}</span>
      </div>

      {/* Flows (RULED: the list lives here; SessionPicker retires). */}
      <Section title={`Flows (${sessions.length})`}>
        <ul className="dp-flows">
          {sessions.map((entry) => (
            <li
              key={entry.id}
              className={
                "dp-flow-row" + (entry.id === activeSessionId ? " active" : "")
              }
            >
              <button
                type="button"
                className="dp-flow-select"
                onClick={() => setActiveSession(entry.id)}
                title="Select this flow — frames, variables, and the transport scope to it"
              >
                {entry.label}
              </button>
              {entry.id !== DEFAULT_SESSION_ID && (
                <button
                  type="button"
                  className="dp-x"
                  title="Close this flow"
                  aria-label={`Close ${entry.label}`}
                  onClick={() => closeSession(entry.id)}
                >
                  ×
                </button>
              )}
            </li>
          ))}
        </ul>
        <div className="dp-flow-add">
          <button
            type="button"
            className="dp-mini"
            title="Open a new session (independent — isolated globals)"
            onClick={() => openSession()}
          >
            + session
          </button>
          <button
            type="button"
            className="dp-mini"
            title="Open a new flow (concurrent — shares globals)"
            onClick={() => openFlow()}
          >
            + flow
          </button>
        </div>
      </Section>

      {/* Frames — interactive call stack (F5). */}
      <Section title={`Frames (${frames.length})`}>
        {frames.length === 0 ? (
          <p className="sv-empty">empty</p>
        ) : (
          <ol className="sv-stack">
            {frames.map((f, i) => {
              const line =
                f.position && resolveDebugLine
                  ? resolveDebugLine(f.position.container_idx, f.position.offset)
                  : null;
              return (
                <li
                  key={i}
                  className={
                    "sv-frame dp-frame" + (i === effectiveFrameIdx ? " selected" : "")
                  }
                >
                  <button
                    type="button"
                    className="dp-frame-head"
                    title={
                      f.position
                        ? "Select this frame — its locals show below; its line gets the accent band"
                        : "Select this frame (no position to reveal)"
                    }
                    onClick={() => {
                      selectFrame(i === 0 ? null : i);
                      if (f.position) revealPosition(f.position);
                    }}
                  >
                    <span className={"sv-badge sv-frame-" + f.kind}>{f.kind}</span>
                    <span className="sv-path">{f.location ?? "—"}</span>
                    {line !== null && (
                      <span className="dp-frame-line sv-dim">
                        {line.file.split("/").pop()}:{line.line + 1}
                      </span>
                    )}
                  </button>
                </li>
              );
            })}
          </ol>
        )}
      </Section>

      {/* Variables — the selected frame's locals, then globals (F6;
          editing is W16). */}
      <Section title="Variables">
        {selectedFrame ? (
          <>
            <p className="dp-subhead sv-dim">
              locals — {selectedFrame.location ?? selectedFrame.kind}
            </p>
            <FrameLocals
              frame={selectedFrame}
              frameIdx={effectiveFrameIdx}
              editable={paused && debugState.status !== "waiting_for_choice"}
              editDisabledReason={
                !paused
                  ? "Pause to edit values (RULED: editing is paused-only)"
                  : "Locals can't be edited at a choice stop — choosing restores the choice's captured thread, which would overwrite the edit"
              }
            />
          </>
        ) : (
          <p className="sv-empty">no frame</p>
        )}
        <p className="dp-subhead sv-dim">globals ({debugState.globals.length})</p>
        {debugState.globals.length === 0 ? (
          <p className="sv-empty">none</p>
        ) : (
          <table className="sv-table">
            <tbody>
              {debugState.globals.map((g) => (
                <tr key={g.name} className={changedGlobals.has(g.name) ? "sv-changed-row" : ""}>
                  <td className="sv-key">{g.name}</td>
                  <td className="sv-val sv-mono">
                    {globalIsScalar(g.value) ? (
                      <EditableScalar
                        display={g.value}
                        disabled={!paused}
                        disabledReason="Pause to edit values (RULED: editing is paused-only)"
                        commit={(input) => debugEditGlobal(g.name, input)}
                      />
                    ) : (
                      g.value
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Section>

      {/* Breakpoints (F2) — the source anchors, program-wide. */}
      <Section
        title={`Breakpoints (${sourceBreakpoints.length})`}
        actions={
          sourceBreakpoints.length > 0 ? (
            <>
              <button
                type="button"
                className="dp-mini"
                title="Disable all breakpoints"
                onClick={breakpointsDisableAll}
              >
                disable all
              </button>
              <button
                type="button"
                className="dp-mini"
                title="Remove all breakpoints"
                onClick={breakpointsClearAll}
              >
                clear
              </button>
            </>
          ) : undefined
        }
      >
        {sourceBreakpoints.length === 0 ? (
          <p className="sv-empty">none — click the editor gutter to set one</p>
        ) : (
          <ul className="dp-breakpoints">
            {sourceBreakpoints.map((b) => (
              <li key={b.key} className="dp-bp-row">
                <input
                  type="checkbox"
                  checked={b.enabled}
                  title={b.enabled ? "Disable" : "Enable"}
                  onChange={(e) => breakpointSetEnabled(b.key, e.target.checked)}
                />
                <button
                  type="button"
                  className={"dp-bp-label" + (b.address === null ? " sv-dim" : "")}
                  title={
                    b.address !== null
                      ? "Bound — click to reveal"
                      : "Unbound (no statement on this line, or no debug info)"
                  }
                  onClick={() => {
                    if (b.address !== null) revealPosition(b.address);
                  }}
                >
                  {b.file.split("/").pop()}:{b.line + 1}
                </button>
                <button
                  type="button"
                  className="dp-x"
                  title="Remove breakpoint"
                  aria-label={`Remove breakpoint at ${b.file}:${b.line + 1}`}
                  onClick={() => breakpointRemove(b.key)}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
      </Section>

      {/* Story (collapsed) — the old StateView's inspection content. */}
      <Section title="Story" defaultOpen={false}>
        <div className="sv-row">
          <span className="sv-key">turn</span>
          <span className="sv-val">{debugState.turn_index}</span>
        </div>
        {debugState.pending_choices.length > 0 && (
          <>
            <p className="dp-subhead sv-dim">
              pending choices ({debugState.pending_choices.length})
            </p>
            <ol className="sv-choices">
              {debugState.pending_choices.map((c, i) => (
                <li key={i} className="sv-row">
                  <span className="sv-val">{c.text}</span>
                  {c.target && <span className="sv-path sv-dim">→ {c.target}</span>}
                </li>
              ))}
            </ol>
          </>
        )}
        <p className="dp-subhead sv-dim">visits ({debugState.visit_counts.length})</p>
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
        <p className="dp-subhead sv-dim">rng</p>
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

// ── Collapsible section (StateView's, plus header actions + default) ──

function Section({
  title,
  children,
  actions,
  defaultOpen = true,
}: {
  title: string;
  children: React.ReactNode;
  actions?: React.ReactNode;
  defaultOpen?: boolean;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="sv-section">
      <div className="dp-section-bar">
        <button
          type="button"
          className={"sv-section-header" + (open ? " open" : "")}
          onClick={() => setOpen((o) => !o)}
        >
          <span className="sv-chevron">{open ? "▾" : "▸"}</span>
          {title}
        </button>
        {actions !== undefined && <span className="dp-section-actions">{actions}</span>}
      </div>
      {open && <div className="sv-section-body">{children}</div>}
    </div>
  );
}

export const DebuggerPanel = memo(DebuggerPanelInner);
