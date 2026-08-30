import { Fragment, useCallback, useEffect, useRef, type ReactNode } from "react";
import {
  EDITOR_MAXIMIZE_GROUP_COMMAND_ID,
  documentKey,
  findTab,
  useEditorGroups,
  useShell,
  type CommandRegistry,
  type DocumentRef,
  type DocumentViewProps,
  type EditorGroupsStore,
} from "@brink/studio-shell";
import { sessionCanContinue } from "@brink/studio-store";
import { useStudioStore } from "./StoreContext.js";

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
): () => void {
  return commands.register({
    id: OPEN_PLAYER_COMMAND_ID,
    title: "Story: Open Player",
    run: () => {
      const state = editorGroups.getState();
      const isOpen = findTab(state.groups, documentKey(playerRef())) !== null;
      const collapsedToSingleNonEmptyGroup =
        state.groups.length === 1 && state.groups[0].tabs.length > 0;
      if (!isOpen && collapsedToSingleNonEmptyGroup) {
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

// ── Character colors ────────────────────────────────────────────

// Cast colors route through the semantic theme tokens (spec §7.4) so the
// screenplay styling stays legible in every theme — no hardcoded hex.
const RAINBOW = [
  "var(--bs-error)",
  "var(--bs-syn-number)",
  "var(--bs-syn-enum)",
  "var(--bs-success)",
  "var(--bs-accent)",
  "var(--bs-syn-keyword)",
];

const CHARACTER_COLORS: Record<string, string | "rainbow"> = {
  // The Toppled Temple cast
  GRISWOLD: "var(--bs-syn-enum)", // gold — the sardonic merchant
  SPECTRE: "var(--bs-info)", // pale cyan — the riddling ghost
  GRIK: "var(--bs-success)", // green — the nervous goblin
  WARDEN: "var(--bs-fg-muted)", // stone grey — the golem
};

/** Render a name with each letter in a cycling rainbow. */
function rainbowName(name: string): ReactNode {
  return (
    <span style={{ fontWeight: 700 }}>
      {name.split("").map((ch, i) => (
        <span key={i} style={{ color: RAINBOW[i % RAINBOW.length] }}>{ch}</span>
      ))}
    </span>
  );
}

/** Render a character name with its color. */
function renderName(name: string): ReactNode {
  const upper = name.toUpperCase();
  const colorDef = CHARACTER_COLORS[upper];
  if (colorDef === "rainbow") {
    return rainbowName(name);
  }
  const color = colorDef ?? "var(--bs-fg)";
  return <span style={{ fontWeight: 700, color }}>{name}</span>;
}

/**
 * Parse a story line and return styled content.
 *
 * The compiled output from screenplay sigils looks like:
 *   @NAME:(parenthetical)Dialogue text here.
 *   @NAME:Dialogue text here.
 *
 * We split these into colored name, italic parenthetical, and dialogue.
 */
function renderLine(line: string): ReactNode {
  // Choice echo: "> text"
  if (line.startsWith("> ")) {
    return <span style={{ color: "var(--bs-accent)" }}>{line}</span>;
  }

  // Screenplay line: @NAME:(paren)dialogue  or  @NAME:dialogue
  const screenplayMatch = line.match(/^@([^:]+):(.*)/);
  if (screenplayMatch) {
    const name = screenplayMatch[1].trim();
    const rest = screenplayMatch[2];

    const parts: ReactNode[] = [<Fragment key="name">{renderName(name)}</Fragment>];

    // Check if rest starts with a parenthetical: (text)remainder
    const parenMatch = rest.match(/^\(([^)]*)\)(.*)/);
    if (parenMatch) {
      parts.push(
        <br key="br1" />,
        <span key="paren" style={{ fontStyle: "italic", color: "var(--bs-fg-muted)" }}>
          ({parenMatch[1]})
        </span>,
      );
      const dialogue = parenMatch[2].trim();
      if (dialogue) {
        parts.push(<br key="br2" />, <span key="dialogue">{dialogue}</span>);
      }
    } else {
      const dialogue = rest.trim();
      if (dialogue) {
        parts.push(<br key="br1" />, <span key="dialogue">{dialogue}</span>);
      }
    }

    return <>{parts}</>;
  }

  // Narrator text — italic, slightly dimmer
  return <span style={{ fontStyle: "italic", color: "var(--bs-fg-muted)" }}>{line}</span>;
}

// ── Component ───────────────────────────────────────────────────

function PlayerPane({ groupId, active }: DocumentViewProps) {
  // Session-bound document view (spec §7.6): selects from the story session
  // and never mutates it — every interaction dispatches a command. The
  // session data/commands are the entire contract (decision log 2026-06-10):
  // the component must never hold or receive the wasm runner handle, so a
  // future SessionProvider (#127) can back it without rework.
  const status = useStudioStore((s) => s.sessionStatus);
  const text = useStudioStore((s) => s.sessionText);
  const choices = useStudioStore((s) => s.sessionChoices);
  const auto = useStudioStore((s) => s.sessionAuto);
  const setSessionAuto = useStudioStore((s) => s.setSessionAuto);
  // Hidden when the bound provider cannot switch modes (the flow provider only
  // ever advances one line) — a visible control that does nothing is worse
  // than no control (#3011).
  const canAuto = useStudioStore((s) => s._provider?.capabilities.has("auto") ?? false);
  // Transport (W5/#3298): render the debug cluster only for a debug-capable
  // provider (disabled-not-hidden while running, hidden entirely for an
  // observe-only provider — same posture as the Auto toggle above).
  const debugCapable = useStudioStore((s) => s.debugCapable);
  const paused = useStudioStore((s) => s.sessionPaused);
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

  const ended = status === "ended" || status === "error";
  const hasPending = sessionCanContinue(status);

  const playerRef = useRef<HTMLDivElement>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when text changes
  useEffect(() => {
    if (playerRef.current) {
      playerRef.current.scrollTop = playerRef.current.scrollHeight;
    }
  }, [text, choices, ended, hasPending]);

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

  // No session: placeholder with a start affordance instead of stale content.
  if (status === "none") {
    return (
      <div className="player-pane" ref={rootRef} tabIndex={-1}>
        <div className="header">
          <div className="toolbar" />
        </div>
        <div className="player">
          <div className="session-placeholder">
            <p className="session-placeholder-title">No story session</p>
            <p className="session-placeholder-hint">
              Start the story to play it here.
            </p>
            <button className="session-placeholder-start" onClick={handleStart}>
              Start story
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="player-pane" ref={rootRef} tabIndex={-1}>
      <div className="header">
        <div className="toolbar">
          <button
            className="player-transport-btn player-btn-run"
            title="Run — compile and start the story"
            aria-label="Run"
            onClick={handleRun}
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <path d="M2.5 1.5l8 4.5-8 4.5z" fill="currentColor" />
            </svg>
          </button>
          <button
            className="player-transport-btn"
            title="Restart the story"
            aria-label="Restart"
            onClick={handleRestart}
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true">
              <path d="M10 6a4 4 0 1 1-1.2-2.8" stroke="currentColor" strokeWidth="1.3" />
              <path d="M9 1v2.5H6.5" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
            </svg>
          </button>
          {canAuto && (
            <button
              className={"player-transport-btn player-auto-btn" + (auto ? " active" : "")}
              title={
                auto
                  ? "Auto reveal on: each reveal runs to the next choice or pause"
                  : "Auto reveal off: each reveal advances a single line"
              }
              aria-label="Auto reveal"
              aria-pressed={auto}
              onClick={() => setSessionAuto(!auto)}
            >
              <svg width="13" height="13" viewBox="0 0 13 13" aria-hidden="true">
                <path d="M1.5 2l4.5 4.5-4.5 4.5z" fill="currentColor" />
                <path d="M6.5 2L11 6.5 6.5 11z" fill="currentColor" />
              </svg>
            </button>
          )}
          {debugCapable && (
            <span className="player-transport">
              <span className="player-transport-sep" />
              {paused ? (
                <button
                  className="player-transport-btn"
                  title="Continue — run to the next line of content and resume play"
                  aria-label="Continue"
                  onClick={() => commands.dispatch("debug.continue")}
                >
                  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
                    <path d="M1.5 2h1.6v8H1.5z" fill="currentColor" />
                    <path d="M5 1.5l6 4.5-6 4.5z" fill="currentColor" />
                  </svg>
                </button>
              ) : (
                <button
                  className="player-transport-btn"
                  title="Pause — stop at the current line; step from there"
                  aria-label="Pause"
                  disabled={status !== "running" && status !== "awaiting-choice"}
                  onClick={() => commands.dispatch("debug.pause")}
                >
                  <svg width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
                    <path d="M2.5 1.5h2.4v9H2.5zM7.1 1.5h2.4v9H7.1z" fill="currentColor" />
                  </svg>
                </button>
              )}
              <button
                className="player-transport-btn"
                title="Step over — one line, calls run to completion"
                aria-label="Step over"
                disabled={!paused}
                onClick={() => commands.dispatch("debug.stepOver")}
              >
                <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
                  <path d="M2 7a5 5 0 0 1 9-2.5" stroke="currentColor" strokeWidth="1.4" />
                  <path d="M11.5 1.5v3.2H8.3" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
                  <circle cx="7" cy="11" r="1.6" fill="currentColor" />
                </svg>
              </button>
              <button
                className="player-transport-btn"
                title="Step into — one line, descending into calls"
                aria-label="Step into"
                disabled={!paused}
                onClick={() => commands.dispatch("debug.stepInto")}
              >
                <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
                  <path d="M7 1.5v6" stroke="currentColor" strokeWidth="1.4" />
                  <path d="M4.2 5l2.8 3 2.8-3" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
                  <circle cx="7" cy="11.5" r="1.6" fill="currentColor" />
                </svg>
              </button>
              <button
                className="player-transport-btn"
                title="Step out — run until the current frame returns"
                aria-label="Step out"
                disabled={!paused}
                onClick={() => commands.dispatch("debug.stepOut")}
              >
                <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
                  <path d="M7 8.5v-6" stroke="currentColor" strokeWidth="1.4" />
                  <path d="M4.2 5l2.8-3 2.8 3" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round" />
                  <circle cx="7" cy="11.5" r="1.6" fill="currentColor" />
                </svg>
              </button>
            </span>
          )}
          <button
            className="player-transport-btn player-btn-maximize"
            onClick={() =>
              commands.dispatch(EDITOR_MAXIMIZE_GROUP_COMMAND_ID, groupId)
            }
            title={maximized ? "Restore (Esc)" : "Maximize"}
          >
            {maximized ? "\u25a3" : "\u25a1"}
          </button>
        </div>
      </div>
      {paused && (
        <div className="player-status-strip">
          <span className="player-status-chip paused" title="Paused by the debugger">
            <span className="player-status-dot" />
            {pausedLocation ? `Paused — ${pausedLocation}` : "Paused"}
          </span>
        </div>
      )}
      <div className="player" ref={playerRef}>
        <div className="story-text">
          {text.map((line, i) => (
            <p key={i}>{renderLine(line)}</p>
          ))}
          {ended && <div className="end-marker">{"— End —"}</div>}
        </div>
        {/* Choices win over Continue: whenever a choice list is present, show
            it and never the Continue button — so a transient status wobble at a
            choice point can't flicker the two against each other (#273). */}
        {choices.length > 0 ? (
          <div className="choices">
            {choices.map((choice) => (
              <button key={choice.index} onClick={() => handleChoice(choice.index)}>
                {choice.text}
              </button>
            ))}
          </div>
        ) : hasPending ? (
          <div className="choices">
            <button onClick={handleContinue}>Continue</button>
          </div>
        ) : null}
      </div>
    </div>
  );
}

export { PlayerPane };
