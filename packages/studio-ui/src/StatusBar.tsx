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
import { ElementTypeEnum, sessionDegraded } from "@brink/studio-store";

// ── Element labels ─────────────────────────────────────────────────

const ELEMENT_LABELS: Record<number, string> = {
  0: "Knot Header",
  1: "Stitch Header",
  2: "Narrative",
  3: "Choice",
  4: "Choice Body",
  5: "Gather",
  6: "Divert",
  7: "Logic",
  8: "Variable",
  9: "Comment",
  10: "Include",
  11: "External",
  12: "Tag",
  13: "Blank",
  14: "Character",
  15: "Parenthetical",
  16: "Dialogue",
};

function elementLabel(info: LineInfo): string {
  let label = ELEMENT_LABELS[info.type] ?? "Unknown";
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
