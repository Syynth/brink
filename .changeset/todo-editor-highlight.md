---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

TODO author notes are now visibly highlighted in the editor (#3050). Lines opening with the `TODO` keyword (colon optional, matching the parser's `AUTHOR_WARNING` rule) classify as the new `todo` element kind and carry the `brink-todo` line class; the opening keyword gets a `brink-todo-keyword` mark. The studio styles the class as a full-width amber band with a left bar, forcing syntax-token colors to the amber inside the note so the line reads as one called-out unit (`--bs-todo`/`--bs-todo-rgb` override per theme, falling back to the warning family). The `E189` squiggle is suppressed in the editor — the band is that diagnostic's in-editor presentation — and Info/Hint diagnostics now map to CodeMirror's `info` severity instead of rendering as warning squiggles.
