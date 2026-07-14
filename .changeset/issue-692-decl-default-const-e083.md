---
"@brink-lang/web": patch
---

Fixed #692: a scalar `VAR`/`CONST` declaration default whose *whole*
value is a non-constant reference or call (`VAR x = someOtherVar`,
`VAR x = f()` — including either wrapped in a prefix/infix operation,
e.g. `VAR x = -f()`) previously folded silently to `Null` through
`eval_const_expr`'s `Path` (`SymbolKind::Variable`) arm and its
catch-all, with zero diagnostic. This is the same silent-fold bug
#673/#679 fixed one level down inside array/map/struct declaration-
default literals (`E075`/`E076`/`E077`), left unfixed at this bare
top-level scalar position.

Observable through `@brink-lang/web`: compiling such a declaration
default now surfaces a real, non-suppressible compile error (`E083`)
instead of silently producing a `Null` global. A `VAR`/`CONST`
referencing another `CONST`, or a `Path` reference nested *inside* a
collection/struct/fn literal, is unaffected (the latter remains the
pre-existing, separately-tracked gap #679's scope notes named).
