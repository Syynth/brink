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

Two of those consumers are **not** diagnostics-only, and both change
observable behavior:

- **`or`-coalescing changes emitted bytecode.** The analyzer records a
  `CoalesceShape` per chain step, which reaches `lir::lower::expr`'s
  `lower_coalesce_chain` through `coalesce_lir_lookup`. A chain whose
  left-hand operand is an unannotated lambda param shadowing an outer
  binding previously recorded `PreserveOption`/`Collapse` derived from the
  *outer* binding's type — the wrong binding, so the wrong code. It now
  records `RuntimeCheck`, which is the honest posture for an operand whose
  Option-ness is not knowable at that point.

- **UFCS receivers can now be a hard error where the code previously
  compiled.** "Unclassifiable" means silence for E071/E078/E116/E117/E152,
  but a UFCS receiver with no knowable type is `E142` ("annotate the
  receiver"). An *unannotated* lambda param used as a method receiver, whose
  name shadows an outer binding, previously resolved from that outer binding
  and compiled; it now raises `E142`. This makes the shadowing case agree
  with the already-existing `E142` for any other unannotated receiver, and
  the fix is to annotate the lambda parameter.
