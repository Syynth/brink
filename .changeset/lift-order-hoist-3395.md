---
"@brink-lang/web": patch
---

Lifting an inline conditional or sequence out of a content line now
evaluates every interpolation to its left first, in source order, into a
hidden compiler-minted temp, so the construct's condition (or a shuffle's
draw) runs after the prefix's side effects, where ink runs it (#3395).
`{bump()}{n == 1:yes|no}` prints `1yes` as the reference does (was `1no`),
and `{n}{bump() == 1:yes|no}` prints `0yes` (was `1yes`). A prefix call that
prints keeps its text inline. Debugger frames' `locals` entries carry a new
`synthetic: boolean`, `true` for those hidden temps (named `$liftN`); the
studio hides them.
