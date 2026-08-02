---
"@brink-lang/web": patch
---

Issue #2004: `!name` line-start sigil dispatch, the self-announcing half
of the §9.1 conventions-handler dispatch split. A content line beginning
`!name` now dispatches by name (or its `@[element(name = "alias")]`
override) to a top-level `fn` annotated `@[element(args = "…")]`, binding
the pattern's named captures to the handler's params by name and
rewriting the line to exactly one call — the same mechanism `claims =
"…"` natural-notation dispatch already uses, minus the pattern match.

Composes with `\!`, the ruled line-start escape (§8d.6) — an escaped `!`
never opens a dispatch. A dispatch whose name is undeclared, whose
remainder doesn't match the handler's pattern, or whose remainder isn't
wholly literal falls through to the existing loud `E129` ("parses
cleanly but has no HIR lowering yet") rather than silently reading as
plain prose.

Not in this slice: dispatching to a `flow` target (only a top-level `fn`
dispatches, matching `claims`'s own restriction), the `block` capture's
dispatch mechanism (issue #1839), cross-file dispatch-name resolution,
and the ruled duplicate-dispatch-name/unmatched-remainder diagnostics
(both interim — first-declared-wins and the generic `E129` fallback,
respectively — pending a diagnostic-code allocation).
