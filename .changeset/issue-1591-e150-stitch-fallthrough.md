---
"@brink-lang/web": patch
---

Analyzer: E150 fall-through check no longer false-positives when the
value-returning `return` lives in a stitch (issue #1591).

`=== function f(): int === / = compute / ~ return 5` previously raised
`E150` ("declares a return type but body never returns a value") even
though the function runs correctly — `check_def`'s E150 path only read
the knot's own `BodyTypes.has_value_return`, missing a `return <expr>`
reached purely by falling through into a stitch. Issue #1551 fixed this
exact blind spot for the E065/E066 escape check, and #1054/PR #1585 fixed
it again for E067's inferred-void collection; this closes the third and
last copy.

`E150` fall-through and `E067` inferred-void now share one
`has_value_return_over_stitches` reading instead of each carrying its own
copy. Also settles a previously unruled question in
`docs/typed-mode-spec.md` §3: "the body" of a function/knot/stitch, for
`E150`/`E067` return-value purposes, is the def's own block *plus* its
stitches — a stitch is reachable by fall-through and is part of the same
definition's execution.

The `E065`/`E066` return-type escape check is deliberately **not** part of
this merge: it reads the def's own inferred return-type signature
(`sig.return_ty`), which is computed per-def and is never merged over
stitches, so it keeps reading the def's own body's has-value-return fact
rather than the merged one.
