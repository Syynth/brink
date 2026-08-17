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

This makes new hard E116 errors (under `types = strict`) appear on
`.brink` files with such a condition, in both the studio Problems panel
and through `EditorSession`/`IdeSnapshot::analyze`.
