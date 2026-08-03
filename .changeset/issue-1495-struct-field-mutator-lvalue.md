---
"@brink-lang/web": patch
---

Fix #1495: `push`/`insert`/`remove_at`/`remove`/… on a struct-field lvalue
(`push(a.items, 3)`, `a: Bag`, `Bag.items: Array<int>`) used to compile
clean and silently misroute the mutation onto the *root* variable instead
of the field, faulting at runtime with `NotIndexable("record")` — a bare
`ident.ident` chain always parses as one multi-segment `hir::Expr::Path`
(never `hir::Expr::FieldAccess`), and the mutator's bare-variable fast path
resolved that whole path's range to the root symbol.

`try_lower_mutator_stmt`'s lvalue dispatch now mirrors
`try_lower_field_assignment`'s existing split: a single-segment path (or
one that doesn't resolve to a struct-field root) keeps the bare-variable
fast path unchanged; a single-level struct-field projection (`a.items`)
routes through a new `lower_field_mutator` (take root → `RecordSet` the
mutated field → write back — the same discipline `p.field = v` already
uses); a **chained** projection (`o.inner.items`, 3+ segments) is rejected
with the same non-suppressible `E074` `try_lower_field_assignment` already
raises for a chained field *write*, rather than falling through to the
same silent misroute.
