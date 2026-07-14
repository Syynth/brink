---
"@brink-lang/web": patch
---

Fixed #743: a bare `VAR` reference *nested one level inside* a
`VAR`/`CONST` declaration-default collection/struct/`#fn` literal — an
array element, a map value, a struct field, or an `#fn(name, args…)`
bound `val` arg — previously folded silently to `Null` through
`eval_const_expr`'s `Path` (`SymbolKind::Variable`) arm, with zero
diagnostic. This is the residue #679's scope notes flagged and #692/
`E083` deliberately left alone (`E083` governs only the *whole*
top-level default, not a construct nested one level in).

Observable through `@brink-lang/web`: compiling such a nested `VAR`
reference (array element / map value / `#fn` bound `val` arg) now
surfaces the existing, non-suppressible `E077` — the same code
`#673`/`#679` already use for any other never-constant nested element
kind — instead of silently producing a `Null` entry. A struct field
was already covered (any struct literal used as a declaration default
is unconditionally `E075`, regardless of field content). A `Path`
reference resolving to a `CONST`/list item/knot/stitch/function is
unaffected — it still folds for real.
