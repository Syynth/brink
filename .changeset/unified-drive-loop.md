---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

Play and debug are one loop (W5/#3298). `debugRun`/`debugStep` — and the
new `debugStepLine` (the author-tier source-line step, bounded by armed
breakpoints) — now return the emitted-lines delta, drained from the SAME
delivery cursor the journaled `continue` road hands lines out of, so a
line the production lookahead already completed surfaces exactly once
whichever loop advances past it. The studio Player routes reveals through
the debug verbs whenever breakpoints are armed or the session is paused;
`pause` is a first-class verb (Player transport: pause/continue + step
over/into/out with a "Paused — location" chip); choices stay journaled,
so restore/replay is unchanged.
