import { useCallback, useEffect, useRef, type ReactNode } from "react";
import { useStudioStore } from "./StoreContext.js";

// ── Character colors ────────────────────────────────────────────

const RAINBOW = ["#f38ba8", "#fab387", "#f9e2af", "#a6e3a1", "#89b4fa", "#cba6f7"];

const CHARACTER_COLORS: Record<string, string | "rainbow"> = {
  // Ratchet's Lair cast
  CHARLOTTE: "rainbow",
  ELIJAH: "#fab387",   // orange/peach
  NOAH: "#f38ba8",     // red
  VIOLET: "#89dceb",   // cyan
  RATCHET: "#c4a882",  // cinnamon
  // Codetta cast
  MINNIE: "#f5c2e7",   // pink
  PARENTS: "#6c7086",  // dim
  CODETTA: "#cba6f7",  // mauve
  JACKIE: "#a6e3a1",   // green
  IRENE: "#f9e2af",    // yellow
  OSCAR: "#fab387",    // peach
  TILDE: "#89dceb",    // teal
  PATTY: "#f38ba8",    // red
  DORIAN: "#94e2d5",   // teal
  SOLSTICE: "#f5c2e7", // pink
  BIJOU: "#cba6f7",    // mauve
  LALA: "#a6e3a1",     // green
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

    const parts: ReactNode[] = [renderName(name)];

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

function PlayerPane() {
  const text = useStudioStore((s) => s.playerText);
  const choices = useStudioStore((s) => s.playerChoices);
  const ended = useStudioStore((s) => s.playerEnded);
  const hasPending = useStudioStore((s) => s._pendingLines.length > 0);
  const chooseOption = useStudioStore((s) => s.chooseOption);
  const resetStory = useStudioStore((s) => s.resetStory);
  const compile = useStudioStore((s) => s.compile);
  const revealNext = useStudioStore((s) => s.revealNext);
  const fullscreen = useStudioStore((s) => s.playerFullscreen);
  const toggleFullscreen = useStudioStore((s) => s.togglePlayerFullscreen);

  const playerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when text changes
  useEffect(() => {
    if (playerRef.current) {
      playerRef.current.scrollTop = playerRef.current.scrollHeight;
    }
  }, [text, choices, ended, hasPending]);

  const handleRun = useCallback(() => {
    compile();
  }, [compile]);

  const handleRestart = useCallback(() => {
    resetStory();
  }, [resetStory]);

  const handleChoice = useCallback(
    (index: number) => {
      chooseOption(index);
    },
    [chooseOption],
  );

  const handleContinue = useCallback(() => {
    revealNext();
  }, [revealNext]);

  return (
    <div id="player-pane">
      <div className="header">
        <span>Story</span>
        <div className="toolbar">
          <button id="btn-run" onClick={handleRun}>
            Run
          </button>
          <button id="btn-restart" onClick={handleRestart}>
            Restart
          </button>
          <button onClick={toggleFullscreen} title={fullscreen ? "Exit fullscreen" : "Fullscreen"}>
            {fullscreen ? "\u25a3" : "\u25a1"}
          </button>
        </div>
      </div>
      <div id="player" ref={playerRef}>
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
