---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

`currentPath()` (#3389 follow-up): the knot or `knot.stitch` the story is
executing in — ink's `currentPathString` without weave indices — on
`StoryRunnerHandle`, `StorySessionHandle` and `FlowHandle`. As in ink it
is where the story IS, so read it before a continue to know where the
coming line is from. The studio's session provider now steps one line
per call on every road and stamps each transcript row with that path,
and the Player ends a speaker's run when consecutive rows come from
different knots or stitches — narration after a divert no longer reads
as the last speaker's lines.
