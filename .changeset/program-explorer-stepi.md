---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Program Explorer additions (W9/#3302). Instruction stepping (`stepi`
into/over/out) lives in the explorer's header — the granularity ladder's
programmer-assist tier, never in the Player toolbar. The
current-instruction highlight follows the Debugger panel's selected
stack frame, not just the top (degraded still suppresses). The editor's
line context menu gains "Reveal in Program Explorer" (the inverse of the
`.inkt` open): the line's instructions open, auto-expanded, scrolled to,
and flashed — with honest notices when no session is running or the line
compiles to nothing. The editor package exposes the
`onRevealInstructions` callback on its play-from-here options.
