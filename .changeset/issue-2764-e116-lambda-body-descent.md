---
"@brink-lang/web": patch
---

Fix #2764: the E116 `Option[T]`-condition-truthiness walk
(`option_conditions.rs`) now descends into a lambda's own block body,
wherever the lambda literal sits — a VAR/CONST default, a temp
initializer, an assignment, a return value, a divert/tunnel/thread-start
argument, a content interpolation, or a condition expression itself — the
same descent `walk_expr_for_lambdas` added for E113 (#2762). Previously an
`if`/`while`/choice condition on an `Option[T]` value sitting inside a
lambda's own `|…| { … }` body was silently unchecked, unlike the identical
condition written at top level.

Only *statically classifiable* conditions fire: a captured outer local, a
global, or a direct Option-returning intrinsic call. A binding the lambda
itself introduces (its own param, or a name its own block binds) is not
classifiable from the enclosing def's finalized locals and is pruned out
of the lookup before the block is checked, so it stays silently unchecked
(the `RuntimeError::OptionTruthiness` runtime fault remains the backstop)
rather than misclassified as a same-named outer binding.

This makes new hard E116 errors (under `types = strict`) appear on
`.brink` files with a *captured* Option condition sitting inside a
lambda's own body, in both the studio Problems panel and through
`EditorSession`/`IdeSnapshot::analyze`.
