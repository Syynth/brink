---
"@brink-lang/web": patch
---

Fixed (#674): the brink-dialect assignment-target grammar now recognizes
an `Index` base for a `.field` write — `arr[i].field = v` parses as a real
assignment target instead of failing with a generic "expression is missing
an operand" (E015) parse error. The compiler still rejects this shape (a
chained/mixed field write, T1e) but now reports the intended `E074`
diagnostic — "chained field-write projection (p.a.b = v) is not
supported" — pointing at the target expression, matching the diagnostic
`o.inner.v = 2.0` already got. Observable through editor/compile
diagnostics for `.ink` source under `dialect = brink`; no change to
`p.field = v` (single-level, still lowers via RMW take/make_mut/write-back)
or to plain `arr[i] = v` indexed assignment.
