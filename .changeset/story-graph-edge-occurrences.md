---
"@brink-lang/web": minor
---

Story-graph edges now carry source spans (#371): each `StoryGraphEdge` lists
its `occurrences` — the divert sites that produced it, as UTF-16 spans
(`{file, start, end}`), one entry per site on aggregated edges. Path targets
anchor on the target path's span; `-> DONE`/`-> END` on the divert statement.
New `StoryGraphEdgeOccurrence` type exported; the field is optional and
omitted only for synthesized diverts with no source anchor.
