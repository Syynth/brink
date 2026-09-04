---
"@brink-lang/editor": patch
---

Structural rails (#3501, ruled 2026-09-03): the rails gutter now mounts as
the RIGHTMOST gutter, directly adjacent to `.cm-content` (after line
numbers and the play/breakpoint/host gutters), via CodeMirror's own gutter
precedence (`Prec.lowest`) rather than CSS reordering. Hovering the column
at a line now shows ONE tooltip listing every container in that line's
stack, outermost first, instead of a tooltip per bar — the per-bar
`pointerenter`/`pointerleave` handlers are gone; the listener lives on the
gutter marker's wrapper. `RAIL_LANE_WIDTH_PX` shrinks from 7 to 5 to match
the bars packing with no gap between them.
