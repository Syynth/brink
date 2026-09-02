import { Fragment, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  EDITOR_MAXIMIZE_GROUP_COMMAND_ID,
  EDITOR_REVEAL_COMMAND_ID,
  encodeProgramAddress,
  documentKey,
  findTab,
  useEditorGroups,
  useShell,
  type CommandRegistry,
  type ShellLayoutStore,
  type DocumentRef,
  type DocumentViewProps,
  type EditorGroupsStore,
} from "@brink/studio-shell";
import {
  sessionCanContinue,
  sessionDegraded,
  type TranscriptLine,
} from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";
import { foldPlayerRuns, speakerPaletteIndex, type PlayerRow } from "./player-runs.js";
import { loadPlayerSettings, savePlayerSettings } from "./SettingsDocument.js";
import { PlayerLauncher } from "./PlayerLauncher.js";

// ── Document type (issue #120, spec §4, §7.6, §7.8) ─────────────────
//
// The Player is an editor-area document, not a tool window: a play session
// is content-you-open, and multi-session (§7.6) maps to player *tabs*. The
// type is session-bound and singleton — one stable DocumentRef over "the
// active session", so reopening focuses the existing tab wherever it lives
// (groups-store reveal policy) and only an explicit split duplicates the
// view (both copies are plain store subscribers over the same session).

export const PLAYER_TYPE_ID = "player";
export const PLAYER_DOC_ID = "session";
export const OPEN_PLAYER_COMMAND_ID = "story.openPlayer";

/** The singleton DocumentRef — one stable identity, one tab per group. */
export function playerRef(): DocumentRef {
  return { typeId: PLAYER_TYPE_ID, docId: PLAYER_DOC_ID, title: "Player" };
}

/**
 * Register `story.openPlayer` (palette: "Story: Open Player" — also reachable
 * from the hamburger menu's "Story" group, since it carries no `when` gate).
 * A plain open reveals an existing tab wherever it lives (the groups store's
 * reveal policy).
 *
 * #280: closing the player tab collapses its split back down to one group
 * (`closeTab`'s last-tab-in-a-group behavior). If the player is closed *and*
 * the editor area is down to one group that still holds other content (the
 * split collapsed, leaving the entry file behind — not a brand new, still-
 * empty editor area with nothing open yet), reopening restores the
 * fresh-load layout (the Inky two-up, `openPlayerSplit` below) instead of
 * dropping the tab into whatever the focused group happens to hold. Once
 * another split already exists (or the lone group is genuinely empty —
 * nothing has been opened at all) there is no "missing" split layout to
 * restore, so it falls through to the normal reveal/open-in-focused-group
 * policy untouched.
 */
export function registerOpenPlayerCommand(
  commands: CommandRegistry,
  editorGroups: EditorGroupsStore,
  layout?: ShellLayoutStore,
): () => void {
  return commands.register({
    id: OPEN_PLAYER_COMMAND_ID,
    title: "Story: Open Player",
    run: () => {
      const state = editorGroups.getState();
      const isOpen = findTab(state.groups, documentKey(playerRef())) !== null;
      const collapsedToSingleNonEmptyGroup =
        state.groups.length === 1 && state.groups[0].tabs.length > 0;
      // #2795: the split restore honors #280's "when there is room" —
      // at the narrow tier there is none, so reopen in the focused group.
      const narrow = layout?.getState().tier === "narrow";
      if (!isOpen && collapsedToSingleNonEmptyGroup && !narrow) {
        openPlayerSplit(editorGroups);
        return;
      }
      editorGroups.getState().openDocument(playerRef(), { pinned: true });
    },
  });
}

/**
 * The default-layout half of the Inky two-up (spec §4): open the player in
 * a split immediately right of the focused group, then hand focus back so
 * typing keeps going to the editor. Called once at bootstrap, after the
 * entry ink file opens in the first group.
 */
export function openPlayerSplit(editorGroups: EditorGroupsStore): void {
  const entryGroupId = editorGroups.getState().focusedGroupId;
  editorGroups.getState().openDocument(playerRef(), {
    group: "split-right",
    pinned: true,
  });
  editorGroups.getState().focusGroup(entryGroupId);
}

// ── Line rendering (#3389, RULED 2026-08-30) ─────────────────────────
//
// Delivered lines render from the PROJECT's dialogue dialect — the same
// resolved artifact the editor classifies with — folded into runs by
// `foldPlayerRuns` (a cue header once, its spoken lines beneath, action and
// narrative outside). No dialect ⇒ plain lines. Nothing about any one
// convention is hardcoded here any more: the previous `@NAME:` regex and
// the demo cast's colour table are gone; speakers get a deterministic
// palette index from their name.

const SPEAKER_PALETTE_SIZE = 6;

/** A row's body: the cue's own text is dropped from a `character` row's
 *  first line (the header carries it); a parenthetical segment renders
 *  in its own style; everything else is the segment text as delivered. */
export function renderRowBody(row: PlayerRow): ReactNode {
  if (row.segments.length === 0) return row.line.text;
  return (
    <>
      {row.segments.map((seg, i) => {
        if (i === 0 && seg.kind === "character") return null;
        if (seg.kind === "parenthetical") {
          return (
            <span key={i} className="player-run-paren">
              {seg.text}
            </span>
          );
        }
        return <Fragment key={i}>{seg.text}</Fragment>;
      })}
    </>
  );
}

// ── Tags toggle persistence (W7/#3300 F13 — off by default, persisted) ──

const TAGS_KEY = "brink-studio.player.show-tags.v1";

// Toolbar glyphs, shared between the inline buttons and the overflow
// menu's rows (maintainer feedback: collapsed controls keep their icons).
function PauseIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M2.5 1.5h2.4v9H2.5zM7.1 1.5h2.4v9H7.1z" fill="currentColor" />
    </svg>
  );
}
function ContinueIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <path d="M1.5 2h1.6v8H1.5z" fill="currentColor" />
      <path d="M5 1.5l6 4.5-6 4.5z" fill="currentColor" />
    </svg>
  );
}
function StepOverIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <path d="M2 7a5 5 0 0 1 9-2.5" stroke="currentColor" strokeWidth="1.4" />
      <path d="M11.5 1.5v3.2H8.3" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
      <circle cx="7" cy="11" r="1.6" fill="currentColor" />
    </svg>
  );
}
function StepIntoIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <path d="M7 1.5v6" stroke="currentColor" strokeWidth="1.4" />
      <path d="M4.2 5l2.8 3 2.8-3" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
      <circle cx="7" cy="11.5" r="1.6" fill="currentColor" />
    </svg>
  );
}
function StepOutIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <path d="M7 8.5v-6" stroke="currentColor" strokeWidth="1.4" />
      <path d="M4.2 5l2.8-3 2.8 3" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
      <circle cx="7" cy="11.5" r="1.6" fill="currentColor" />
    </svg>
  );
}
/** Follow — lines with an arrow: the editor tracks the Player. */
function FollowIcon() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M4 6h16M4 12h9M4 18h16" />
      <path d="M17 9l3 3-3 3" />
    </svg>
  );
}

function FastForwardIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 13 13" aria-hidden="true">
      <path d="M1.5 2l4.5 4.5-4.5 4.5z" fill="currentColor" />
      <path d="M6.5 2L11 6.5 6.5 11z" fill="currentColor" />
    </svg>
  );
}
function SaveStateIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
      <path d="M2 2h6.5L10 3.5V10H2z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
      <path d="M4 2v2.6h3.4V2M4 10V7h4v3" stroke="currentColor" strokeWidth="1.1" />
    </svg>
  );
}
function StopIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
      <rect x="2.5" y="2.5" width="7" height="7" rx="1" fill="currentColor" />
    </svg>
  );
}
function TagsIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
      <path
        d="M1.5 5V1.5H5L10.5 7 7 10.5z"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinejoin="round"
      />
      <circle cx="3.6" cy="3.6" r="0.9" fill="currentColor" />
    </svg>
  );
}

function loadTagsToggle(): boolean {
  try {
    return localStorage.getItem(TAGS_KEY) === "1";
  } catch {
    return false;
  }
}

function saveTagsToggle(on: boolean): void {
  try {
    localStorage.setItem(TAGS_KEY, on ? "1" : "0");
  } catch {
    // localStorage may be unavailable; the toggle still works in-session.
  }
}

/** Trailing path segment — the hover chip shows `name.ink:12`, not the
 * full project-relative path. */
function baseName(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx === -1 ? path : path.slice(idx + 1);
}

// ── Component ───────────────────────────────────────────────────

function PlayerPane({ groupId, active }: DocumentViewProps) {
  // Session-bound document view (spec §7.6): selects from the story session
  // and never mutates it — every interaction dispatches a command. The
  // session data/commands are the entire contract (decision log 2026-06-10):
  // the component must never hold or receive the wasm runner handle, so a
  // future SessionProvider (#127) can back it without rework.
  const status = useStudioStore((s) => s.sessionStatus);
  const followInEditor = useStudioStore((s) => s.followInEditor);
  const followPaused = useStudioStore((s) => s.followPaused);
  const setFollowInEditor = useStudioStore((s) => s.setFollowInEditor);
  const setSessionHoverSource = useStudioStore((s) => s.setSessionHoverSource);
  const showProvenance = useStudioStore((s) => s.showProvenance);
  const showChoiceMarkers = useStudioStore((s) => s.showChoiceMarkers);
  const lines = useStudioStore((s) => s.sessionLines);
  const projectDialect = useStudioStore((s) => s.projectDialect);
  const groups = useMemo(() => foldPlayerRuns(lines, projectDialect), [lines, projectDialect]);
  // Out-of-sync gate (spec §5): degraded suppresses provenance and the
  // chip goes warning — suppressed, never stale.
  const degraded = useStudioStore((s) =>
    sessionDegraded(s.programChecksum, s.compiledChecksum),
  );
  const resolveSourceBytes = useStudioStore((s) => s._resolveSourceBytes);
  const saveCurrentState = useStudioStore((s) => s.saveCurrentState);
  const choices = useStudioStore((s) => s.sessionChoices);
  // One-shot fast-forward (RULED 2026-08-30) — no sticky auto mode.
  const revealMaximally = useStudioStore((s) => s.revealMaximally);
  // Transport (W5/#3298): render the debug cluster only for a debug-capable
  // provider (disabled-not-hidden while running, hidden entirely for an
  // observe-only provider — same posture as the Auto toggle above).
  const debugCapable = useStudioStore((s) => s.debugCapable);
  const paused = useStudioStore((s) => s.sessionPaused);
  // W18: a watchpoint stop names the written global in the chip.
  const lastOutcome = useStudioStore((s) => s.debugLastOutcome);
  const pausedLocation = useStudioStore((s) => {
    // file:line when the resolver can say (W6/#3299) — the shape every
    // debugger user reads — falling back to the runtime's knot path.
    const pos = s.debugState?.position;
    const provider = s._provider;
    if (pos && provider !== null && "resolveDebugLine" in provider) {
      const line = (
        provider as { resolveDebugLine(c: number, o: number): { file: string; line: number } | null }
      ).resolveDebugLine(pos.container_idx, pos.offset);
      if (line !== null) return `${line.file}:${line.line + 1}`;
    }
    return s.debugState?.current_location ?? null;
  });
  const { commands } = useShell();
  const maximized = useEditorGroups((s) => s.maximizedGroupId) === groupId;
  // Tags toggle (F13): muted mono chips per line; off by default, persisted.
  const [showTags, setShowTags] = useState(loadTagsToggle);
  // Only the hovered row computes its provenance chip — the byte→editor
  // walk reads the whole file, so it must never run per-row per-render.
  const [hoverIdx, setHoverIdx] = useState(-1);
  // Auto-scroll suspends while the author reads back (scrolled up), and
  // resumes when they return to the bottom (rebuild housekeeping).
  const stickToBottom = useRef(true);
  // Toolbar collapse (RULED 2026-08-30): when the pane is too narrow,
  // whole sub-sections fold into an overflow menu ONE GROUP AT A TIME —
  // level 1 folds the transport cluster, level 2 the secondary controls
  // (FF / Stop / Save / tags). Run, Restart, the chip and Maximize stay.
  const toolbarRef = useRef<HTMLDivElement>(null);
  const [collapse, setCollapse] = useState(0);
  const [overflowOpen, setOverflowOpen] = useState(false);
  const expandAt = useRef<number[]>([]);
  useEffect(() => {
    const el = toolbarRef.current;
    if (!el) return;
    const check = (): void => {
      const overflowing = el.scrollWidth > el.clientWidth + 1;
      setCollapse((c) => {
        if (overflowing && c < 2) {
          // Remember the width we gave up at — re-expand only with slack,
          // so the boundary doesn't flap.
          expandAt.current[c] = el.clientWidth + 56;
          return c + 1;
        }
        if (!overflowing && c > 0 && el.clientWidth >= (expandAt.current[c - 1] ?? Infinity)) {
          return c - 1;
        }
        return c;
      });
    };
    // jsdom has no ResizeObserver — measure once and skip live tracking.
    if (typeof ResizeObserver === "undefined") {
      check();
      return;
    }
    const ro = new ResizeObserver(check);
    ro.observe(el);
    check();
    return () => ro.disconnect();
  }, []);
  // Content changes overflow the toolbar without resizing it — a collapse
  // step folding a group, but ALSO ordinary re-renders growing a control
  // (found live: the status chip widening to "Waiting on choice" while
  // the width held steady left a permanent overflow the ResizeObserver
  // never saw). Re-measure after EVERY render; the check is two property
  // reads, and setCollapse with an unchanged value schedules nothing.
  useEffect(() => {
    const el = toolbarRef.current;
    if (!el) return;
    if (el.scrollWidth > el.clientWidth + 1) {
      setCollapse((c) => {
        if (c >= 2) return c;
        expandAt.current[c] = el.clientWidth + 56;
        return c + 1;
      });
    }
  });

  const ended = status === "ended" || status === "error";
  const hasPending = sessionCanContinue(status);

  const playerRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom on new content — unless the author scrolled up.
  useEffect(() => {
    if (playerRef.current && stickToBottom.current) {
      playerRef.current.scrollTop = playerRef.current.scrollHeight;
    }
  }, [lines, choices, ended, hasPending]);

  const handleScroll = useCallback(() => {
    const el = playerRef.current;
    if (!el) return;
    stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
  }, []);

  // Provenance reveal (F9): a transcript row jumps back to its source —
  // the Player-side twin of the breakpoint gutter. Degraded suppresses.
  const revealSource = useCallback(
    (line: TranscriptLine) => {
      if (degraded || !line.source || !resolveSourceBytes) return;
      const point = resolveSourceBytes(
        line.source.file,
        line.source.range_start,
        line.source.range_end,
      );
      if (point === null) return;
      commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
        kind: "source",
        file: line.source.file,
        span: { start: point.start, end: point.end },
      });
    },
    [commands, degraded, resolveSourceBytes],
  );

  // Becoming the focused group's active tab takes DOM focus into the pane,
  // exactly as a revealed text document focuses its CM6 view. Without this,
  // a command that reveals the player (story.openPlayer via the palette)
  // loses the focus fight: the palette overlay's focus-return runs after the
  // command and hands DOM focus back to the editor, whose focusin handler
  // snaps the focused group right back. Effects run after unmount cleanups
  // in the same commit, so this focus lands last and wins.
  useEffect(() => {
    const root = rootRef.current;
    if (!active || root === null) return;
    if (root.contains(document.activeElement)) return;
    root.focus({ preventScroll: true });
  }, [active]);

  const handleRun = useCallback(() => {
    commands.dispatch("compile.run");
  }, [commands]);

  const handleRestart = useCallback(() => {
    commands.dispatch("story.restart");
  }, [commands]);

  const handleChoice = useCallback(
    (index: number) => {
      commands.dispatch("story.choose", index);
    },
    [commands],
  );

  const handleContinue = useCallback(() => {
    commands.dispatch("story.continue");
  }, [commands]);

  const handleStart = useCallback(() => {
    commands.dispatch("story.start");
  }, [commands]);

  const position = useStudioStore((s) => s.debugState?.position ?? null);
  // W15/#3308: a successful hot-reload flashes a brief affirmation in
  // the chip (the ruled minimal UI); the tick clears it after ~2s.
  const reloadedAt = useStudioStore((s) => s.sessionReloadedAt);
  const [, bumpReloadTick] = useState(0);
  const justReloaded = reloadedAt !== null && Date.now() - reloadedAt < 2000;
  useEffect(() => {
    if (!justReloaded) return;
    const t = setTimeout(() => bumpReloadTick((n) => n + 1), 2100);
    return () => clearTimeout(t);
  }, [justReloaded, reloadedAt]);

  // The status chip — the single home of stop reasons (spec §3): ready /
  // playing / paused at file:line / waiting on choice / ended / error /
  // out-of-sync. Clicking reveals the current line in the editor.
  const chip = degraded
    ? { cls: "degraded", label: "Out of sync" }
    : justReloaded && status !== "none"
      ? { cls: "playing", label: "Reloaded" }
      : status === "none"
      ? { cls: "ready", label: "Ready" }
      : paused
        ? {
            cls: "paused",
            label:
              lastOutcome?.reason.type === "watchpoint"
                ? `Paused on write — ${lastOutcome.reason.name ?? `g${lastOutcome.reason.global_idx.toString()}`}`
                : pausedLocation
                  ? `Paused — ${pausedLocation}`
                  : "Paused",
          }
        : status === "awaiting-choice"
          ? { cls: "waiting", label: "Waiting on choice" }
          : status === "ended"
            ? { cls: "ended", label: "Ended" }
            : status === "error"
              ? { cls: "error", label: "Error" }
              : { cls: "playing", label: "Playing" };

  const revealCurrent = useCallback(() => {
    if (degraded || position === null) return;
    commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
      kind: "program",
      address: encodeProgramAddress(position.container_idx, position.offset),
    });
  }, [commands, degraded, position]);

  // Idle (RULED "no auto-start", W7/#3300): the toolbar (Run + the Ready
  // chip) stays, the body is the launcher placeholder — replaced by
  // W14's saves launcher.
  const idle = status === "none";

  return (
    <div className="player-pane" ref={rootRef} tabIndex={-1}>
      <div className="header">
        <div className="toolbar" ref={toolbarRef}>
          <button
            className="player-transport-btn player-btn-run"
            data-tip="Run — compile and start the story"
            aria-label="Run"
            onClick={handleRun}
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <path d="M2.5 1.5l8 4.5-8 4.5z" fill="currentColor" />
            </svg>
          </button>
          <button
            className="player-transport-btn"
            data-tip="Restart the story"
            aria-label="Restart"
            onClick={handleRestart}
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
              <path d="M10 6a4 4 0 1 1-1.2-2.8" stroke="currentColor" strokeWidth="1.3" />
              <path d="M9 1v2.5H6.5" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
            </svg>
          </button>
          {collapse < 2 && (
          <>
          <button
            className="player-transport-btn"
            data-tip="Stop the story — back to the launcher"
            aria-label="Stop"
            disabled={idle}
            onClick={() => commands.dispatch("story.stop")}
          >
            <StopIcon />
          </button>
          <button
            className="player-transport-btn"
            data-tip="Save state — checkpoint the current point (W14); writes back to a loaded save"
            aria-label="Save state"
            disabled={idle}
            onClick={() => void saveCurrentState()}
          >
            <SaveStateIcon />
          </button>
          <button
            className="player-transport-btn player-auto-btn"
            data-tip="Fast-forward — run to the next choice or stop (one shot; paced per Settings → Player)"
            aria-label="Fast-forward"
            disabled={idle}
            onClick={() => revealMaximally()}
          >
            <FastForwardIcon />
          </button>
          <button
            className={
              "player-transport-btn player-auto-btn player-follow-btn" +
              (followInEditor ? " active" : "") +
              (followInEditor && followPaused ? " is-paused" : "")
            }
            data-tip={
              followInEditor
                ? followPaused
                  ? "Follow in editor — paused while you edit; click to resume (Run or Restart resumes too)"
                  : "Follow in editor — the editor scrolls to each revealed line (click to stop)"
                : "Follow in editor — off (click to follow the story in the editor)"
            }
            aria-label="Follow in editor"
            aria-pressed={followInEditor}
            onClick={() => {
              const next = !(followInEditor && !followPaused);
              setFollowInEditor(next);
              savePlayerSettings(window.localStorage, {
                ...loadPlayerSettings(window.localStorage),
                followInEditor: next,
              });
            }}
          >
            <FollowIcon />
          </button>
          {debugCapable && collapse < 1 && (
            <span className="player-transport">
              <span className="player-transport-sep" />
              {paused ? (
                <button
                  className="player-transport-btn"
                  data-tip="Continue — run to the next line of content and resume play"
                  aria-label="Continue"
                  onClick={() => commands.dispatch("debug.continue")}
                >
                  <ContinueIcon />
                </button>
              ) : (
                <button
                  className="player-transport-btn"
                  data-tip="Pause — stop at the current line; step from there"
                  aria-label="Pause"
                  disabled={status !== "running" && status !== "awaiting-choice"}
                  onClick={() => commands.dispatch("debug.pause")}
                >
                  <PauseIcon />
                </button>
              )}
              <button
                className="player-transport-btn"
                data-tip="Step over — one line, calls run to completion"
                aria-label="Step over"
                disabled={!paused}
                onClick={() => commands.dispatch("debug.stepOver")}
              >
                <StepOverIcon />
              </button>
              <button
                className="player-transport-btn"
                data-tip="Step into — one line, descending into calls"
                aria-label="Step into"
                disabled={!paused}
                onClick={() => commands.dispatch("debug.stepInto")}
              >
                <StepIntoIcon />
              </button>
              <button
                className="player-transport-btn"
                data-tip="Step out — run until the current frame returns"
                aria-label="Step out"
                disabled={!paused}
                onClick={() => commands.dispatch("debug.stepOut")}
              >
                <StepOutIcon />
              </button>
            </span>
          )}
          <button
            className={"player-transport-btn player-tags-btn" + (showTags ? " active" : "")}
            data-tip={showTags ? "Hide line tags" : "Show line tags"}
            aria-label="Show tags"
            aria-pressed={showTags}
            onClick={() => {
              setShowTags((on) => {
                saveTagsToggle(!on);
                return !on;
              });
            }}
          >
            <TagsIcon />
          </button>
          </>
          )}
          {collapse > 0 && (
            <span className="player-overflow">
              <button
                className={"player-transport-btn" + (overflowOpen ? " active" : "")}
                data-tip="More controls"
                aria-label="More controls"
                aria-expanded={overflowOpen}
                onClick={() => setOverflowOpen((o) => !o)}
              >
                {"⋯"}
              </button>
              {overflowOpen && (
                <div
                  className="player-overflow-menu"
                  onKeyDown={(e) => {
                    if (e.key === "Escape") setOverflowOpen(false);
                  }}
                >
                  {debugCapable && (
                    <>
                      <button
                        className="player-overflow-item"
                        disabled={!paused && status !== "running" && status !== "awaiting-choice"}
                        onClick={() => {
                          commands.dispatch(paused ? "debug.continue" : "debug.pause");
                          setOverflowOpen(false);
                        }}
                      >
                        {paused ? <ContinueIcon /> : <PauseIcon />}
                        <span>{paused ? "Continue" : "Pause"}</span>
                      </button>
                      <button
                        className="player-overflow-item"
                        disabled={!paused}
                        onClick={() => {
                          commands.dispatch("debug.stepOver");
                          setOverflowOpen(false);
                        }}
                      >
                        <StepOverIcon />
                        <span>Step over</span>
                      </button>
                      <button
                        className="player-overflow-item"
                        disabled={!paused}
                        onClick={() => {
                          commands.dispatch("debug.stepInto");
                          setOverflowOpen(false);
                        }}
                      >
                        <StepIntoIcon />
                        <span>Step into</span>
                      </button>
                      <button
                        className="player-overflow-item"
                        disabled={!paused}
                        onClick={() => {
                          commands.dispatch("debug.stepOut");
                          setOverflowOpen(false);
                        }}
                      >
                        <StepOutIcon />
                        <span>Step out</span>
                      </button>
                    </>
                  )}
                  {collapse >= 2 && (
                    <>
                      <button
                        className="player-overflow-item"
                        disabled={idle}
                        onClick={() => {
                          revealMaximally();
                          setOverflowOpen(false);
                        }}
                      >
                        <FastForwardIcon />
                        <span>Fast-forward</span>
                      </button>
                      <button
                        className="player-overflow-item"
                        disabled={idle}
                        onClick={() => {
                          void saveCurrentState();
                          setOverflowOpen(false);
                        }}
                      >
                        <SaveStateIcon />
                        <span>Save state</span>
                      </button>
                      <button
                        className="player-overflow-item"
                        disabled={idle}
                        onClick={() => {
                          commands.dispatch("story.stop");
                          setOverflowOpen(false);
                        }}
                      >
                        <StopIcon />
                        <span>Stop</span>
                      </button>
                      <button
                        className="player-overflow-item"
                        onClick={() => {
                          setShowTags((on) => {
                            saveTagsToggle(!on);
                            return !on;
                          });
                          setOverflowOpen(false);
                        }}
                      >
                        <TagsIcon />
                        <span>{showTags ? "Hide line tags" : "Show line tags"}</span>
                      </button>
                    </>
                  )}
                </div>
              )}
            </span>
          )}
          <button
            className={"player-status-chip " + chip.cls}
            title="Reveal the current line in the editor"
            onClick={revealCurrent}
          >
            <span className="player-status-dot" />
            {chip.label}
          </button>
          <button
            className="player-transport-btn player-btn-maximize"
            onClick={() =>
              commands.dispatch(EDITOR_MAXIMIZE_GROUP_COMMAND_ID, groupId)
            }
            data-tip={maximized ? "Restore (Esc)" : "Maximize"}
          >
            {maximized ? "\u25a3" : "\u25a1"}
          </button>
        </div>
      </div>
      <div className="player" ref={playerRef} onScroll={handleScroll}>
        {idle && <PlayerLauncher />}
        <div className={"player-spine" + (idle ? " is-idle" : "")}>
        {/* The beginning of the timeline (maintainer, 2026-09-02): a node at
            the head of the rail, so the rail reads as a line of play from
            the first line, not as a bracket around whatever is on screen. */}
        <div className="player-start-marker" aria-hidden="true">
          <span className="player-start-label">Start</span>
        </div>
        <div className="story-text">
          {groups.map((group, gi) => {
            const renderRow = (row: PlayerRow): ReactNode => {
              const { line, index: i } = row;
              const point =
                i === hoverIdx && !degraded && line.source && resolveSourceBytes
                  ? resolveSourceBytes(
                      line.source.file,
                      line.source.range_start,
                      line.source.range_end,
                    )
                  : null;
              // A choice echo whose kind the wire knows (#3435) draws its
              // `*`/`+` as a ring on the spine and drops the textual `> `.
              const echoKind = line.kind === "marker" ? line.choiceKind : undefined;
              const point1 = point !== null && line.source ? point.line + 1 : null;
              return (
                <div
                  key={i}
                  className={
                    `player-line-row kind-${line.kind}` +
                    (row.kind !== null ? ` dialect-${row.kind}` : "") +
                    (echoKind !== undefined ? ` is-echo echo-${echoKind}` : "")
                  }
                  onMouseEnter={() => {
                    setHoverIdx(i);
                    // Hover → editor (#3437): band the row's source line.
                    setSessionHoverSource(line.source ?? null);
                  }}
                  onMouseLeave={() => {
                    setHoverIdx((cur) => (cur === i ? -1 : cur));
                    setSessionHoverSource(null);
                  }}
                  onClick={(e) => {
                    // ⌘/Ctrl-click anywhere on the row jumps to source (F9).
                    if (e.metaKey || e.ctrlKey) revealSource(line);
                  }}
                >
                  {echoKind !== undefined && showChoiceMarkers && (
                    <span className="player-echo-ring" aria-hidden="true">
                      {echoKind === "sticky" ? "+" : "*"}
                    </span>
                  )}
                  <p>{echoKind !== undefined ? line.text.replace(/^>\s*/, "") : renderRowBody(row)}</p>
                  {showTags && line.tags.length > 0 && (
                    <span className="player-line-tags">
                      {line.tags.map((tag, ti) => (
                        <code key={ti} className="player-tag-chip">
                          #{tag}
                        </code>
                      ))}
                    </span>
                  )}
                  {showProvenance && point1 !== null && line.source && (
                    <button
                      className="player-provenance"
                      title={`${baseName(line.source.file)}:${point1.toString()} · ⌘-click to open`}
                      aria-label={`Open ${baseName(line.source.file)}:${point1.toString()} in the editor`}
                      onClick={(e) => {
                        e.stopPropagation();
                        revealSource(line);
                      }}
                    >
                      <GoToSourceIcon />
                    </button>
                  )}
                </div>
              );
            };
            if (group.speaker === null) {
              return <Fragment key={`g${gi.toString()}`}>{group.rows.map(renderRow)}</Fragment>;
            }
            const palette = speakerPaletteIndex(group.speaker, SPEAKER_PALETTE_SIZE);
            return (
              <div
                key={`g${gi.toString()}`}
                className={`player-run dialect-${group.kind ?? "run"} speaker-${palette.toString()}`}
                data-speaker={group.speaker}
              >
                <p className={`player-run-cue speaker-${palette.toString()}`}>{group.speaker}</p>
                {group.rows.map(renderRow)}
              </div>
            );
          })}
          {ended && <div className="end-marker">{"— End —"}</div>}
        </div>
        {/* Choices win over Continue: whenever a choice list is present, show
            it and never the Continue button — so a transient status wobble at a
            choice point can't flicker the two against each other (#273). */}
        {choices.length > 0 ? (
          <div className="choices">
            {choices.map((choice) => (
              <button
                key={choice.index}
                className={
                  "player-choice" +
                  (choice.sticky === undefined ? "" : choice.sticky ? " is-sticky" : " is-once")
                }
                onClick={() => handleChoice(choice.index)}
              >
                {choice.sticky !== undefined && showChoiceMarkers && (
                  <span className="player-choice-mark" aria-hidden="true">
                    {choice.sticky ? "+" : "*"}
                  </span>
                )}
                <span className="player-choice-text">{choice.text}</span>
              </button>
            ))}
          </div>
        ) : hasPending ? (
          <div className="choices">
            <button className="player-choice player-continue" onClick={handleContinue}>
              <span className="player-choice-text">Continue</span>
            </button>
          </div>
        ) : null}
        </div>
      </div>
    </div>
  );
}

/** The provenance button's glyph — text with an arrow out of it. */
function GoToSourceIcon() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M14 4h6v6M20 4l-8 8M11 6H6a2 2 0 0 0-2 2v10a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-5" />
    </svg>
  );
}

export { PlayerPane };
