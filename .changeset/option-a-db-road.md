---
"@brink-lang/web": patch
---

Option A total (ruled 2026-08-24): the editor's per-edit analysis routes
through the db's incremental `analysis_query` — the `IdeSnapshot` deep
clone that cost ~28–33 ms per keystroke at large-file scale (#3063) is
deleted, and `updateDocument`'s wasm share drops accordingly. Wire-visible
changes: the internal perf counters `ide.snapshotClone`/`ide.applyAnalysis`
retired (`ide.analyze` now measures the incremental pull; compare
recorded runs across the boundary with that in mind), and `getStoryGraph`
returns an empty graph instead of `null` on a fresh session (analysis is
always available now; the `StoryGraph | null` type is kept). Also closes
the #2885 options-sync gap: an equal-options `compileProject` can never
cold-invalidate the live analysis.
