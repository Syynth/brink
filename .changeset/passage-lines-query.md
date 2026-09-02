---
"@brink-lang/web": patch
---

`EditorSession.passage_lines(path)` / `passageLines(path)` (#3408): the
content lines of a knot or stitch across the project — the source line
with weave scaffolding (`*`/`+`/`-`, `(label)`, leading `{condition}`
groups) and tags removed, tags carried separately, with the declaring
file, one-based line and origin (`line` / `choice` / `gather`). Headers,
logic, diverts, declarations, comments and tag-only lines are not lines.
This is what the Conventions editor's teach-by-example marking list pulls.
