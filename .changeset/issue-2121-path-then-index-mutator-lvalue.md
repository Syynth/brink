---
"@brink-lang/web": patch
---

Fix #2121: `push(a.items[0], v)` and `a.items[0] = v` — a **Path-then-Index**
lvalue whose root is itself a struct-field projection (`a.items`, `a: Bag`,
`Bag.items: Array<Array<int>>`/`Array<int>`) — used to compile clean and
silently misroute the write onto the *root* variable `a` instead of
`a.items[0]`, faulting at runtime with `NotIndexable("record")` — the "one
level down" remainder of #1495/PR #2106's fix: a bare `ident.ident` chain
always parses as one multi-segment `hir::Expr::Path` (never
`hir::Expr::FieldAccess`), and wrapping that `Path` in an `Index` reaches
`lower_indexed_assignment`/`lower_lvalue_container_chain` — a different
call chain than #2106's fix, which only taught the *bare* Path-lvalue
dispatch about this shape.

Both call sites now reject this shape with the same non-suppressible
`E074` `try_lower_field_assignment`/`lower_mutator_call` already raise for
a chained field write/mutator, rather than falling through to the same
silent misroute.
