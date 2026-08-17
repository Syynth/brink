---
"@brink-lang/web": patch
---

Analyzer: fix a false-positive/false-negative hazard for expressions inside
a lambda's own block or expression body, under `types = strict` (issue
#2773).

`MistypeCtx.locals`/`BodyTypes::locals` key local bindings by bare name,
with no notion of lexical scope. `hir::visit::walk_expr`'s `Expr::Lambda`
descent (issue #1685) has always walked into a lambda's own body as part of
the ordinary expression tree, so every analyzer pass that classifies a
`Path`/`Call`/`Index` expression from this map while visiting an expression
— `int(x)`/`float(x)` domain checks (E078), `int(r)` range-refinement
(E117), `contains(m, k)` key-domain checks (E152), `or`-coalescing operand
typing, UFCS receiver resolution, and struct-construction field typing
(E071) — was live-exposed to misattributing a lambda's own param or
block-local temp the type of a same-named *outer* binding of a different
type, the moment the lambda body happened to reuse an outer name. A lambda
parameter/temp genuinely shadowing an outer local now classifies from its
own type (or "unclassifiable", never the outer binding's) throughout its
own body, for every one of the checks above.
