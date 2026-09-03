/**
 * The execution highlight (W6/#3299 — RULED 2026-08-29: live during play,
 * not paused-only). "Play is stepping": from the moment a session runs,
 * the editor carries the per-line treatment continuously — the band
 * follows each delivered line as it is revealed, and pause/breakpoints
 * stop the advance without switching anything on.
 *
 * Color language (the design canvas's taxonomy): live = success tint,
 * paused = warning tint (+ the gutter arrow, drawn by the play gutter —
 * the ruled shared column), selected frame = accent tint. Degraded
 * sessions suppress the highlight at the HOST (the host's callback
 * returns null) — suppressed, never stale.
 *
 * Same host contract as the breakpoint dots: the host owns the truth,
 * the editor re-reads ONLY on {@link refreshExecutionHighlight} — no
 * polling, no transaction of its own when the runtime moves.
 */

import { StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";

/** Where execution is, in author-facing terms. */
export interface ExecutionHighlight {
  /** 1-based line the band covers. */
  line: number;
  /** 1-based LAST line of a multi-line band (`≥ line`): every line from
   *  `line` to `endLine` gets the same band — a transcript line built
   *  from several source lines (glue, a cue + aside + dialogue) reads as
   *  one in the Player, so it lights as one here. Omitted = one line. */
  endLine?: number;
  /** Two channels (ruled 2026-09-03): the whole-line TINT is state — where
   *  play is (`live` / `paused` / `frame`, the choice-point set) — and the
   *  inset left BAR is attention (`follow` / `hover` / `peek`), bar-only,
   *  stacking on a tinted line.
   *
   *  live = the line being revealed during play (no gutter glyph) — also
   *  every PRESENTED choice at a choice point (W11/#3304, the plural
   *  case);
   *  paused = where the debugger halted (warning band + gutter arrow);
   *  frame = a selected non-top stack frame (accent band + hollow arrow);
   *  rejected = an authored choice NOT added to the block (dimmed, with
   *  {@link ExecutionHighlight.note} rendered beside the line). */
  kind: "live" | "paused" | "frame" | "rejected" | "follow" | "hover" | "peek";
  /** Why a `rejected` line was left out — rendered as a muted chip at
   *  the line's end (e.g. "once-only · used", "condition false"). For
   *  the by-elimination condition case, the extension enriches the
   *  label with the line's own `{…}` condition text when present. */
  note?: string;
  /**
   * The covering debug entry's exact byte range — reserved for
   * finer-than-line emphasis (expression-level entries, instruction
   * stepping, step-out's mid-line call-site landing). UTF-8 BYTE offsets
   * in the file as the compiler consumed it: a consumer must convert to
   * editor (UTF-16) positions before decorating with them. The v1 line
   * band deliberately ignores them; they ride the seam so those
   * consumers need no new one.
   */
  rangeStart?: number;
  rangeLen?: number;
}

/** Re-read the host's execution highlights and re-render (bands + the
 * play gutter's arrow, which watches {@link executionHighlightVersion}). */
export function refreshExecutionHighlight(view: EditorView): void {
  view.dispatch({ effects: refreshExecutionHighlightEffect.of(null) });
}

const refreshExecutionHighlightEffect = StateEffect.define<null>();

/** Bumped per refresh so gutters (`lineMarkerChange`) have state to
 * compare — the same idiom as the breakpoint version field. */
export const executionHighlightVersion = StateField.define<number>({
  create: () => 0,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(refreshExecutionHighlightEffect)) return value + 1;
    return value;
  },
});

export interface ExecutionHighlightOptions {
  /** ALL current highlights — plural by design: a choice point lights
   *  every presented choice's line at once (W11's ruled visualization),
   *  and a selected stack frame's band coexists with the paused band
   *  (W8). Ordinary play/pause yields zero or one. Empty = nothing. */
  getExecutionHighlights: () => readonly ExecutionHighlight[];
}

export function executionHighlightExtension(options: ExecutionHighlightOptions): Extension {
  function build(state: EditorState): DecorationSet {
    const highlights = options.getExecutionHighlights();
    const decos = highlights
      .filter((h) => h.line >= 1 && h.line <= state.doc.lines)
      .flatMap((h) => {
        const line = state.doc.line(h.line);
        let note = h.note;
        if (h.kind === "rejected" && note === "condition false") {
          // By-elimination enrichment (W11/#3304): show the line's own
          // condition when the source carries one — `* {gold > 20} […]`
          // reads as "gold > 20 = false".
          const m = /^\s*[*+]\s*(?:\(\s*[\w.]+\s*\)\s*)?\{([^}]+)\}/.exec(line.text);
          if (m) note = `${m[1].trim()} = false`;
        }
        const deco = Decoration.line({
          class: `brink-exec-line brink-exec-${h.kind}`,
          attributes: note !== undefined ? { "data-brink-exec-note": note } : undefined,
        });
        const last = Math.min(Math.max(h.endLine ?? h.line, h.line), state.doc.lines);
        const out = [];
        for (let n = h.line; n <= last; n++) out.push(deco.range(state.doc.line(n).from));
        return out;
      });
    // Decoration.set requires sorted ranges; hosts owe no ordering.
    return Decoration.set(decos, true);
  }

  const field = StateField.define<DecorationSet>({
    create: build,
    update(value, tr) {
      if (tr.effects.some((e) => e.is(refreshExecutionHighlightEffect))) return build(tr.state);
      // An edit maps the band along rather than re-reading the host —
      // the host's own change-mapping/refresh lands right behind it.
      return tr.docChanged ? value.map(tr.changes) : value;
    },
    provide: (f) => EditorView.decorations.from(f),
  });

  return [executionHighlightVersion, field, executionHighlightTheme];
}

const executionHighlightTheme = EditorView.baseTheme({
  // The canvas's tints: live 9% success, paused 14% warning, frame 10%
  // accent — full-line bands, subtle enough to read prose through.
  ".brink-exec-live": {
    backgroundColor: "rgb(var(--bs-success-rgb, 34 197 94) / 9%)",
  },
  ".brink-exec-paused": {
    backgroundColor: "rgb(var(--bs-warning-rgb, 234 179 8) / 14%)",
  },
  ".brink-exec-frame": {
    backgroundColor: "rgb(var(--bs-accent-rgb, 59 130 246) / 10%)",
  },
  // The cursor's own active-line highlight and a state tint used to fight
  // for the background; on a tinted line the cursor gets its own colour
  // (ruled 2026-09-03), so both read at once.
  ".cm-activeLine.brink-exec-live, .cm-activeLine.brink-exec-paused, .cm-activeLine.brink-exec-frame":
    {
      backgroundColor:
        "var(--bs-exec-active-line-bg, rgb(var(--bs-fg-rgb, 205 214 244) / 14%))",
    },
  // Attention bars (ruled 2026-09-03): bar-only, no tint of their own.
  // Follow (#3437): the line the Player just revealed — a solid accent bar.
  ".brink-exec-follow": {
    boxShadow: "inset 3px 0 0 var(--bs-accent, #3b82f6)",
  },
  // Hover (#3437): delivered content under the pointer — a solid muted
  // bar, never mistaken for where play is.
  ".brink-exec-hover": {
    boxShadow: "inset 3px 0 0 var(--bs-fg-muted, #6c7086)",
  },
  // Peek: a forecast of what the hovered Continue / choice would hit —
  // a DASHED accent bar, styled as "not yet real", no gutter glyph. Drawn
  // as a pseudo-element so it never widens the line.
  ".brink-exec-peek": {
    position: "relative",
  },
  ".brink-exec-peek::before": {
    content: '""',
    position: "absolute",
    left: "0",
    top: "0",
    bottom: "0",
    width: "3px",
    background:
      "repeating-linear-gradient(to bottom, var(--bs-accent, #3b82f6) 0 4px, transparent 4px 7px)",
    pointerEvents: "none",
  },
  // Rejected choice (W11/#3304): dimmed line, reason chip at line end.
  ".brink-exec-rejected": {
    opacity: "0.55",
  },
  ".brink-exec-rejected::after": {
    content: "attr(data-brink-exec-note)",
    marginLeft: "12px",
    padding: "0 5px",
    fontSize: "85%",
    fontStyle: "italic",
    color: "var(--bs-fg-muted, #888)",
    border: "1px solid var(--bs-border, #444)",
    borderRadius: "3px",
  },
});
