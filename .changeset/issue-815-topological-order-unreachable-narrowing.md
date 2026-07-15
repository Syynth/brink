---
"@brink-lang/web": patch
---

Issue #815 (FG-4 train): `IncludeGraph::topological_order` no longer appends
project files unreachable from the compile entry point as a "shouldn't
happen in practice, but be safe" fallback. Only `entry` and files it
transitively `INCLUDE`s now feed `lir_lowering_query`'s inputs.

Mechanism: `topological_order` used to run a post-order DFS from `entry`,
then append every remaining live file (sorted by `FileId`) that DFS didn't
reach. `brink-driver::discover` (the one-shot CLI/oracle-corpus compile
path) never produces unreached files — it only ever loads `entry` plus its
transitive `INCLUDE` closure — so the fallback was always a no-op there,
which is why the oracle corpus (5,577 episodes) is byte-identical before and
after this change. The fallback mattered only for `ProjectDb`'s other role
as the long-lived editor-session model, where files are added independently
of any single entry point and an unrelated file (or an orphaned one, e.g.
after removing an `INCLUDE`) can coexist with `entry` in the same session
with no `INCLUDE` edge between them at all.

Observable through `@brink-lang/web`: in a multi-file editor session (an
`IdeSession`/equivalent with more than one file loaded and an entry set via
`setEntry`), a file with no `INCLUDE` relationship to the current entry no
longer contributes its globals/lists/externals/knots to the compiled
`StoryData`, and editing that unrelated file no longer invalidates the
entry's compiled LIR. That file's own diagnostics are unaffected — they run
as independent per-file passes (`analysis_diagnostics_query`/
`diagnostics_query`) that were never routed through `topological_order`, so
they still surface exactly as before.

Oracle ratchet unchanged (5,577 episodes, byte-identical) — every corpus
case has a single entry-reachable file set, so this is oracle-inert by
construction.
