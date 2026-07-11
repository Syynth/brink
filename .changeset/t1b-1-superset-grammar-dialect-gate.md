---
"@brink-lang/web": patch
---

Added the T1b superset grammar (#569, docs/t1b-surface-spec.md §§1-4):
multi-line `~ { … }` logic blocks (assignment, `temp`, `if`/`else if`/`else`,
`while`, `for … in …`, `break`/`continue`, `return`, expression statements),
`#[…]`/`#{…}` sigil collection literals in expression position, and postfix
indexing (`a[0]`, chained `grid[y][x]`) plus indexed assignment. Parsed
through CST/AST/HIR; nothing lowers to LIR or codegen yet (lands in T1b-2).

Introduced a compiler dialect gate (`AnalysisOptions::dialect`,
`Dialect::{StrictInk, Brink}`, default `StrictInk`) — a new analysis input,
not embedded in `.inkb`. Under `StrictInk` (the default every existing
caller gets), every extension construct now produces a targeted diagnostic
(`E051`) at its exact span instead of whatever parse/analysis error it
previously produced for that byte sequence. Under `Brink`, the same
constructs produce a "not yet implemented — lands in T1b-2" diagnostic
(`E052`). Both dialects still fail to compile source using this syntax —
this is a diagnostic-quality change for a previously-unsupported syntax
shape, not new compileable output.

Plain ink is unaffected: the oracle corpus remains byte-identical (5,577
passing episodes) since none of it uses the new syntax, and the new grammar
is purely additive (`if`/`while`/`for`/`break`/`continue`/`in` are
contextual keywords, recognized only at block-statement-start position
inside a new `~ { … }` block — they remain ordinary identifiers everywhere
else, so no existing knot/variable/function name is reserved).
