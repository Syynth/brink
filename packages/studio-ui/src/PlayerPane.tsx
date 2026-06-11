import { Fragment, useCallback, useEffect, useRef, type ReactNode } from "react";
import {
  EDITOR_MAXIMIZE_GROUP_COMMAND_ID,
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
 * Register `story.openPlayer` (palette: "Story: Open Player", no default
 * keybinding). Opens pinned into the focused group; the groups store's
 * reveal policy focuses an existing tab wherever it lives.
 */
export function registerOpenPlayerCommand(
  commands: CommandRegistry,
  editorGroups: EditorGroupsStore,
): () => void {
  return commands.register({
    id: OPEN_PLAYER_COMMAND_ID,
    title: "Story: Open Player",
    run: () =>
      editorGroups.getState().openDocument(playerRef(), { pinned: true }),
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

const RAINBOW = ["#f38ba8", "#fab387", "#f9e2af", "#a6e3a1", "#89b4fa", "#cba6f7"];

const CHARACTER_COLORS: Record<string, string | "rainbow"> = {
  // The Toppled Temple cast
  GRISWOLD: "#f9e2af", // gold — the sardonic merchant
  SPECTRE: "#89dceb",  // pale cyan — the riddling ghost
  GRIK: "#a6e3a1",     // green — the nervous goblin
  WARDEN: "#9399b2",   // stone grey — the golem
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
  const color = colorDef ?? "var(--brink-fg)";
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
    return <span style={{ color: "var(--brink-accent)" }}>{line}</span>;
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
        <span key="paren" style={{ fontStyle: "italic", color: "var(--brink-fg-dim)" }}>
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
  return <span style={{ fontStyle: "italic", color: "var(--brink-fg-dim)" }}>{line}</span>;
}

// ── Component ───────────────────────────────────────────────────

function PlayerPane({ groupId }: DocumentViewProps) {
  // Session-bound document view (spec §7.6): selects from the story session
  // and never mutates it — every interaction dispatches a command. The
  // session data/commands are the entire contract (decision log 2026-06-10):
  // the component must never hold or receive the wasm runner handle, so a
  // future SessionProvider (#127) can back it without rework.
  const status = useStudioStore((s) => s.sessionStatus);
  const text = useStudioStore((s) => s.sessionText);
  const choices = useStudioStore((s) => s.sessionChoices);
  const { commands } = useShell();
  const maximized = useEditorGroups((s) => s.maximizedGroupId) === groupId;

  const ended = status === "ended" || status === "error";
  const hasPending = sessionCanContinue(status);

  const playerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when text changes
  useEffect(() => {
    if (playerRef.current) {
      playerRef.current.scrollTop = playerRef.current.scrollHeight;
    }
  }, [text, choices, ended, hasPending]);

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
      <div className="player-pane">
        <div className="header">
          <span>Story</span>
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
    <div className="player-pane">
      <div className="header">
        <span>Story</span>
        <div className="toolbar">
          <button className="btn-run" onClick={handleRun}>
            Run
          </button>
          <button className="btn-restart" onClick={handleRestart}>
            Restart
          </button>
          <button
            onClick={() =>
              commands.dispatch(EDITOR_MAXIMIZE_GROUP_COMMAND_ID, groupId)
            }
            title={maximized ? "Restore (Esc)" : "Maximize"}
          >
            {maximized ? "\u25a3" : "\u25a1"}
          </button>
        </div>
      </div>
      <div className="player" ref={playerRef}>
        <div className="story-text">
          {text.map((line, i) => (
            <p key={i}>{renderLine(line)}</p>
          ))}
          {ended && <div className="end-marker">{"\u2014 End \u2014"}</div>}
        </div>
        {hasPending && (
          <div className="choices">
            <button onClick={handleContinue}>Continue</button>
          </div>
        )}
        {!hasPending && choices.length > 0 && (
          <div className="choices">
            {choices.map((choice) => (
              <button key={choice.index} onClick={() => handleChoice(choice.index)}>
                {choice.text}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export { PlayerPane };
