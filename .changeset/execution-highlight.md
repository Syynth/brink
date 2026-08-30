---
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

The live execution highlight (W6/#3299 — "play is stepping"). The wasm
sessions gain `resolveDebugLine(containerIdx, offset)` — the
position→source road: file, 0-based line, and the covering debug entry's
exact byte range (kept on the seam for future instruction-level
stepping). The editor gains `executionHighlightExtension`, a plural
highlight seam (a choice point or selected stack frame can light several
lines at once): a subtle full-line band per position — green while
playing, amber when paused (with a filled gutter arrow in the shared
play/breakpoint column), accent for a selected frame (hollow arrow). The
studio wires it end-to-end: the band follows every reveal, pausing
scrolls the editor to the stop (reveal-on-stop) and shows a
"Paused — file:line" chip in the Player, and degraded sessions suppress
the highlight rather than showing a stale one.
