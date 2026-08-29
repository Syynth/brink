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
  mirror, interactive Frames (selected frame, kind badges, clickable
  `file:line`, a parked "resumes here" frame), locals-first Variables
  with change highlight, Breakpoints with enable checkboxes, the old
  StateView content demoted to collapsed Story sections.
- `GutterStates.dc.html` — the marker taxonomy in the ONE shared gutter
  column (breakpoints render where "play from here" does; context menu
  resolves header-line conflicts): header-conflict / bound / disabled /
  unbound-hollow / paused-here / selected-frame / parked, with the
  degraded-suppression rules.
- `Transport.dc.html` — toolbar in four states (playing, paused, parked,
  out-of-sync) plus the F-row keybinding legend.
