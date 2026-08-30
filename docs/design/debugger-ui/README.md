# Debugger UI — design canvas

Design round 2026-08-29 (epic #452 D9 + the Player half of #3199). Spec:
`docs/debugger-ui-spec.md`. Rulings: `docs/decision-log.md` 2026-08-29
(debug info on by default; session-only state; StateView replaced; Player
rebuild folded in). Live annotatable canvas: published as the "Brink
Debugger UI" artifact.

One scenario runs through every artboard: paused at a breakpoint on
`~ gold -= cost` inside `=== function pour(n) ===`, called from
`tavern.order`. Catppuccin Mocha `--bs-*` token values throughout.

- `Main.dc.html` — full-studio composite while paused: editor with
  breakpoint gutter + warning-tint current-line band + accent-tint
  selected-frame band, rebuilt Player (right split) with transport +
  status chip + paused transcript marker, Debugger panel in the right
  dock, paused status-bar segment.
- `Player.dc.html` — the rebuilt Player close-up in the *playing* state:
  transport cluster (steps disabled until paused), auto-reveal toggle
  (fast-forward icon, paced playback per App settings), tags toggle with
  per-line chips, zebra line-row boundaries with an incoming paced line,
  status strip, choice buttons, and the transcript provenance affordance
  (hover band + `file:line · ⌘-click to reveal` tooltip).
- `DebuggerPanel.dc.html` — the StateView replacement: header transport
  mirror, a Flows section (open flows with the active one selected and a
  parked flow's "resumes here" state — replaces the status bar's
  SessionPicker), the selected flow's interactive Frames (kind badges,
  clickable `file:line`), locals-first Variables with change highlight
  and an inline value edit in progress (paused-only, scalars v1), the
  Watch mini-REPL (typed expressions + a sandboxed `-> market.haggle`
  transcript preview, over the shipped F4/F5.1 `evaluate()` engine),
  Breakpoints with enable checkboxes plus a break-on-write data
  breakpoint (diamond glyph), the old StateView content demoted to
  collapsed Story sections.
- `GutterStates.dc.html` — the marker taxonomy in the ONE shared gutter
  column (breakpoints render where "play from here" does; context menu
  resolves header-line conflicts): header-conflict / live-line-playing /
  bound / disabled / unbound-hollow / paused-here / selected-frame /
  parked, with the degraded-suppression rules. Color language: live =
  success, paused = warning + arrow, selected frame = accent + hollow
  arrow, parked = info dashed, breakpoints = error.
- `ChoicePoint.dc.html` — the story waiting on a choice: presented
  choice lines lit (success band) in the editor, rejected candidates
  dimmed with their reason (`gold > 20 = false`, `once-only · used` —
  derived from the presented set + visit counts + #3234's anonymous
  container ids, no new runtime seam), the runtime-value hover
  card on `gold`, and the Player showing the presented pair with a
  "Waiting on choice" chip.
- `IdleSaves.dc.html` — the idle Player as the launcher (F17): Run from
  the start beside the play-from-anywhere typeahead (KNOT/STITCH kind
  chips + file context), then saves as two stacked sections in the
  landing screen's Recent-list style — caps "PROJECT" and "THIS
  COMPUTER" over bordered row lists (TURN-count chips, amber OLD chip
  for an older compile), Load/Fork on the hovered save (Load writes
  back, Fork copies to a new slot), and the surfaced LoadReport banner
  ("3 anonymous visit states dropped").
- `Transport.dc.html` — toolbar in five states (ready — the default, the
  story never plays on open; playing; paused; parked; out-of-sync) plus
  the F-row keybinding legend. Play is stepping: one live visualization,
  pause just stops the advance.
