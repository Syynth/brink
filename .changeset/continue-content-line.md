---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

Continue runs to the next content line (2026-08-30 ruling, extending
#3321). The wasm sessions gain `debugRunToLine` — advance until a content
line COMMITS (running through the glue/commit boundary, so the crossed
line is in the outcome at the stop — no one-advance delivery lag), or a
breakpoint/choices/terminal stop comes first; needs no debug line info.
The Player's Continue and the reveal-while-paused click both route
through it and RESUME play on an ordinary stop (band back to live, chip
clears) — an author no longer grinds through `~` statements one click at
a time to reach content. Step Over/Into/Out stay statement-granular for
the programmer tier, and choosing while paused still stays paused (F7's
choice presentation), now delivering the consequence's content line in
the same gesture.
