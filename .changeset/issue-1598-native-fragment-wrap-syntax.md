---
"@brink-lang/web": patch
---

`StoryRunnerHandle.compileFragment` (`evaluate()`'s Tier-1 fragment-compile
step) now picks its synthetic-symbol wrap syntax from the project entry's
dialect instead of hardcoding ink's `=== ===` knot syntax:

- A `.brink` native entry gets native wrap syntax — `fn NAME() { return
  (EXPR); }` for the expression attempt, `flow NAME() { CONTENT }` for the
  content fallback.
- An `.ink` (or extensionless) entry keeps ink's `=== function NAME() ===` /
  `=== NAME ===` wraps, unchanged.

Previously, appending ink knot syntax to a `.brink` entry was a native
parse error, so `evaluate()`'s Tier-1 fragment path could never succeed for
a native project — `compile_fragment` itself was already dialect-agnostic
(#1387/#1595), but its only caller never spoke native syntax. Fixes #1598.
