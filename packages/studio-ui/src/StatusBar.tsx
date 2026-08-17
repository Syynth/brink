/**
 * Status-bar segments (docs/studio-shell-spec.md §7.3).
 *
 * Each segment is a component registered into the shell's StatusBarRegistry
 * at bootstrap (brink-studio/main.tsx) — the shell renders the bar region
 * (ShellStatusBar) without knowing what the segments are. Left group: app
 * status (compile, story session). Right group: editor context (cursor,
 * element type + conversion dropdown, key hints).
 */

import { useCallback, useState } from "react";
import { useShell, viewToggleCommandId } from "@brink/studio-shell";
import { useStudioStore } from "./StoreContext.js";
import { ElementDropdown } from "./ElementDropdown.js";
import type { LineInfo } from "@brink/studio-store";
import { ElementTypeEnum, sessionDegraded, DEFAULT_SESSION_ID } from "@brink/studio-store";

// ── Element labels ─────────────────────────────────────────────────
//
// Keyed by the kebab-case kind string (#368: ElementType is an open string
// union, not a numeric enum). A dialect-declared kind not in this table
// (e.g. a custom dialect's "channel") falls back to a title-cased rendering
// of the kind string, so new kinds are labeled sensibly with zero changes
// here — the open-scheme contract (docs/editor-consumer-guide.md).

const ELEMENT_LABELS: Record<string, string> = {
  [ElementTypeEnum.KnotHeader]: "Knot Header",
  [ElementTypeEnum.StitchHeader]: "Stitch Header",
  [ElementTypeEnum.NarrativeText]: "Narrative",
  [ElementTypeEnum.Choice]: "Choice",
  [ElementTypeEnum.ChoiceBody]: "Choice Body",
  [ElementTypeEnum.Gather]: "Gather",
  [ElementTypeEnum.Divert]: "Divert",
  [ElementTypeEnum.Logic]: "Logic",
  [ElementTypeEnum.VarDecl]: "Variable",
  [ElementTypeEnum.Comment]: "Comment",
  [ElementTypeEnum.Include]: "Include",
  [ElementTypeEnum.External]: "External",
  [ElementTypeEnum.Tag]: "Tag",
  [ElementTypeEnum.Blank]: "Blank",
  [ElementTypeEnum.Character]: "Character",
  [ElementTypeEnum.Parenthetical]: "Parenthetical",
  [ElementTypeEnum.Dialogue]: "Dialogue",
};

/** Title-case a kebab-case kind string as a fallback label for a kind not in
 *  {@link ELEMENT_LABELS} (a dialect-declared kind, e.g. "channel"). */
function titleCaseKind(kind: string): string {
  return kind
    .split("-")
    .map((word) => (word.length > 0 ? word[0].toUpperCase() + word.slice(1) : word))
    .join(" ");
}

function elementLabel(info: LineInfo): string {
  let label = ELEMENT_LABELS[info.type] ?? titleCaseKind(info.type);
  if (
    (info.type === ElementTypeEnum.Choice || info.type === ElementTypeEnum.Gather) &&
    info.depth > 1
  ) {
    label += ` · ${info.depth}`;
  }
  if (info.type === ElementTypeEnum.Choice && info.sticky) {
    label += " (+)";
  }
  return label;
}

// ── Left group ──────────────────────────────────────────────────────

/**
 * Compile status: error/warning counts (or a quiet check when clean).
 * Clicking toggles the Problems tool window (#84) via its generated
 * `view.toggle.problems` command — if that command isn't registered (e.g. a
 * host shell without Problems), the segment renders non-interactive.
 */
export function CompileStatusSegment() {
  const diagnostics = useStudioStore((s) => s.diagnostics);
  const { commands } = useShell();

  const problemsCommand = viewToggleCommandId("problems");
  const clickable = commands.get(problemsCommand) !== undefined;

  let text = "✓";
  let className = "brink-status-diag";
  if (diagnostics.errors > 0 || diagnostics.warnings > 0) {
    const parts: string[] = [];
    if (diagnostics.errors > 0) {
      parts.push(`${diagnostics.errors} error${diagnostics.errors > 1 ? "s" : ""}`);
    }
    if (diagnostics.warnings > 0) {
      parts.push(`${diagnostics.warnings} warning${diagnostics.warnings > 1 ? "s" : ""}`);
    }
    text = parts.join(", ");
    className += diagnostics.errors > 0 ? " has-errors" : " has-warnings";
  }

  if (!clickable) return <span className={className}>{text}</span>;
  return (
    <button className={className + " clickable"} onClick={() => commands.dispatch(problemsCommand)}>
      {text}
    </button>
  );
}

const STORY_STATUS_LABELS: Record<string, string> = {
  none: "no story",
  running: "running",
  "awaiting-choice": "awaiting choice",
  done: "done",
  ended: "ended",
  error: "error",
};

/** Story session status (spec §7.6); click restarts when that makes sense. */
export function StorySegment() {
  const status = useStudioStore((s) => s.sessionStatus);
  // Degraded mode (spec §5, #181): the running program isn't the studio's
  // latest compile. Surfaced as a status, not a notification — source-position
  // features (graph location, visit badges) are disabled while it holds.
  const degraded = useStudioStore((s) =>
    sessionDegraded(s.programChecksum, s.compiledChecksum),
  );
  const { commands } = useShell();

  const label = degraded
    ? "inspecting — source out of sync"
    : (STORY_STATUS_LABELS[status] ?? status);
  const canRestart = commands.isEnabled("story.restart");
  const body = (
    <>
      <span
        className={`brink-status-story-dot status-${status}${degraded ? " degraded" : ""}`}
      />
      {label}
    </>
  );

  const className = `brink-status-story${degraded ? " degraded" : ""}`;
  if (!canRestart) return <span className={className}>{body}</span>;
  return (
    <button
      className={`${className} clickable`}
      title="Restart story"
      onClick={() => commands.dispatch("story.restart")}
    >
      {body}
    </button>
  );
}

/**
 * Local busy-state affordance for a gated structural op running off the
 * paint path — `moveStitch`/`promoteStitch`/`demoteKnot` (#2767,
 * `runGatedStructuralOp` in `symbolMenuActions.ts`) and the Binder's file/
 * folder rename-and-move (#2776, `applyRename` in `binder.ts`); spec §7.7.4.
 * Renders only while `structuralOpPending` is set — quiet, non-modal, and
 * gone the moment the deferred wasm call settles. This is deliberately a
 * status-bar segment and NOT a shell notification: spec §7.5 states progress
 * notifications are out of scope for the notification service, so progress
 * for these one-shot context-menu/drag-drop/rename actions lives here per
 * §7.3 instead.
 */
export function StructuralOpSegment() {
  const pending = useStudioStore((s) => s.structuralOpPending);
  if (pending === null) return null;
  return (
    <span className="brink-status-structural-op" role="status" aria-live="polite">
      {pending}…
    </span>
  );
}

/**
 * Multi-session picker (docs/multi-session-spec.md §5, #182). Lists the
 * registered sessions and repoints every session-bound view to the selected
 * one. Hidden when ≤1 session — no picker noise in the single-session studio;
 * opening the first extra session is the `story.openSession` command.
 */
export function SessionPicker() {
  const sessions = useStudioStore((s) => s.sessions);
  const activeSessionId = useStudioStore((s) => s.activeSessionId);
  const setActiveSession = useStudioStore((s) => s.setActiveSession);
  const openSession = useStudioStore((s) => s.openSession);
  const openFlow = useStudioStore((s) => s.openFlow);
  const closeSession = useStudioStore((s) => s.closeSession);

  if (sessions.length <= 1) return null;

  const canClose = activeSessionId !== null && activeSessionId !== DEFAULT_SESSION_ID;
  return (
    <span className="brink-status-sessions">
      <select
        className="brink-session-select"
        title="Active session"
        value={activeSessionId ?? ""}
        onChange={(e) => setActiveSession(e.target.value)}
      >
        {sessions.map((session) => (
          <option key={session.id} value={session.id}>
            {session.label}
          </option>
        ))}
      </select>
      <button
        className="brink-session-add clickable"
        title="Open a new session (independent — isolated globals)"
        onClick={() => openSession()}
      >
        +
      </button>
      <button
        className="brink-session-add clickable"
        title="Open a new flow (concurrent — shares globals)"
        onClick={() => openFlow()}
      >
        +⑂
      </button>
      {canClose && (
        <button
          className="brink-session-close clickable"
          title="Close this session"
          onClick={() => closeSession(activeSessionId)}
        >
          ×
        </button>
      )}
    </span>
  );
}

// ── Right group ─────────────────────────────────────────────────────

export function CursorSegment() {
  const cursor = useStudioStore((s) => s.cursor);
  return (
    <span className="brink-status-cursor">
      {cursor.line}:{cursor.col}
    </span>
  );
}

/** Element type label + the conversion dropdown (on the overlay primitive). */
export function ElementSegment() {
  const lineInfo = useStudioStore((s) => s.currentLineInfo);
  const convertLine = useStudioStore((s) => s.convertLineToType);

  const [open, setOpen] = useState(false);
  // The anchor is state (not a ref) so the Overlay re-renders once the
  // button exists and repositions via floating-ui's autoUpdate from then on.
  const [anchor, setAnchor] = useState<HTMLButtonElement | null>(null);

  const handleSelect = useCallback(
    (sigil: string) => {
      setOpen(false);
      convertLine(sigil);
    },
    [convertLine],
  );

  return (
    <>
      <button
        ref={setAnchor}
        className="brink-status-element-btn"
        onClick={() => setOpen((v) => !v)}
      >
        {lineInfo ? elementLabel(lineInfo) : "Blank"}
      </button>
      <ElementDropdown
        open={open}
        anchor={anchor}
        onSelect={handleSelect}
        onDismiss={() => setOpen(false)}
      />
    </>
  );
}

export function KeyHintsSegment() {
  const hints = useStudioStore((s) => s.currentLineHints);
  if (hints.length === 0) return null;
  return (
    <span className="brink-status-keyhint">
      {hints.map((h) => `${h.key}: ${h.hint}`).join("  ·  ")}
    </span>
  );
}
