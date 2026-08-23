---
"@brink-lang/studio": minor
---

Player advances one line per reveal, with an "auto" toggle for run-to-pause

`LocalSessionProvider.reveal()` called `continueToPause()` unconditionally, so
a single Continue press dumped every line up to the next choice. Its own doc
comment said it revealed "the next line" — the comment described the intent and
the code did something else.

All three reveal paths (initial load, after a choice, Continue) now advance a
single line. A new `auto` capability + `setAuto()` switches them to
run-to-next-pause, surfaced as an unchecked-by-default checkbox in the Player
toolbar. `SessionSnapshot` gains `auto` so the control reflects provider state
rather than a separate copy that can drift.

Flow sessions honour the toggle too, via `continueFlowMaximally`.
