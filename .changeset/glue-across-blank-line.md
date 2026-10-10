---
"@brink-lang/web": patch
---

Glue now reaches across a blank line, as in ink. `a` / `{e}` / `<> b`, where `{e}` renders empty, prints `a b` on one line, and so does `a <>` / `{e}` / `b`. A glue walks back over blank values and removes every newline until it meets visible text or a tag (#3535). A blank value no longer ends the line before it, so a function that prints `1` and then a blank line, called as `{f()}`, prints `12` as ink does, not `1` and `2` (#3558).
