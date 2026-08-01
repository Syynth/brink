---
"@brink-lang/web": patch
---

Analyzer: `string + int`/`string + float` concatenation no longer reports
`E066` under `types = strict` (issue #1911).

`+` between a `string` and an `int`/`float`, in either operand order, is
ink's core display-concatenation idiom (`"score: " + points`, `keys + ":"
+ total`). The strict checker previously unified `+` as a same-type
operator with no exception for it, so concatenating a numeric value into a
string marked the numeric binding `Conflicted` and reported `E066` on code
that compiles, runs, and produces the correct output today — a false
positive on legal, idiomatic ink. `docs/typed-mode-spec.md` §4 now rules
this pairing as `string`-typed display concatenation, matching the
runtime's own `Add` behavior (`value_ops::binary_op`'s `String`/`Int` and
`String`/`Float` arms, which already stringify the numeric operand
unconditionally). The carve-out is scoped exactly to that runtime
behavior: `Add` only, and `Int`/`Float` only — `string + bool` and
string-numeric `-`/`*`/`/`/`%` still report `E066`, since the runtime
defines no such operation. Covers the `+=` compound-assignment spelling
too (`keys += total`), not just infix `+` — it reaches the runtime's same
`Add` arm through a separate inference seam (`Stmt::Assignment`/
`BlockStmt::Assignment`) that needed the identical carve-out.
