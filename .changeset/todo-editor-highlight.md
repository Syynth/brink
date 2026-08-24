---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

TODO author notes are now visibly highlighted in the editor (#3050). Lines opening with the `TODO` keyword (colon optional, matching the parser's `AUTHOR_WARNING` rule) classify as the new `todo` element kind and carry the `brink-todo` line class; the opening keyword gets a `brink-todo-keyword` mark. The studio styles the class as a full-width amber band with a left bar (`--bs-todo`/`--bs-todo-rgb` override per theme, falling back to the warning family).
