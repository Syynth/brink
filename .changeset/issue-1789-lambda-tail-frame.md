---
"@brink-lang/web": patch
---

Analyzer: a block-bodied lambda's tail expression is inferred inside the
lambda's own frame (issue #1789).

`InferPass::infer_lambda` snapshots and restores the five frame-scoped
fields (`return_ty`, `has_value_return`, `locals`, `annotated`,
`local_fn_origins`) around a block-bodied lambda's body, so the lambda's
own locals never leak into the enclosing definition. The restore was
landing *between* the body's `stmts` and its trailing tail expression,
because the tail was reached through `LambdaBody::value_exprs()` in a loop
that sat after the restore. The tail was therefore inferred against the
enclosing definition's `locals` — and since `locals` is keyed by bare
name, that failed in both directions on a shadowed name:

- a temp declared by the lambda's own statements was invisible to the
  lambda's own tail, so the `E063` arity check (which needs a known
  callee type) was skipped there entirely (an over-applied call through a
  lambda-local `fn` temp in tail position was never checked for arity —
  a spurious `E065` Unknown-escape fired in its place instead, not
  silence);
- a use in argument position in the tail unified its type into whatever
  *enclosing* local shared that bare name, turning e.g. an enclosing `int`
  temp `Conflicted` and reporting a spurious `E066` on a temp the
  enclosing body never misuses.

The frame window now wraps both the statements and the tail, so both
directions stay inside the lambda and are discarded by the restore. Under
`types = strict`, code hitting the second case stops seeing a false-positive
`E066`, and code hitting the first starts seeing the diagnostic it should
always have produced (`E063` arity errors on a call in tail position, in
place of the spurious `E065` it used to get).

A third, incidental direction: a *captured* (not shadowed) enclosing
temp used only from a lambda's tail no longer gets narrowed by that use.
`observe`'s effect from the tail is now confined to the lambda's own
frame and discarded by the restore, same as any other tail-position
write — so an unannotated enclosing temp that previously picked up a
type from a tail-only capturing use now stays unannotated and `E065`-
escapes there too, matching how the same use already behaved from
statement position under #1750. Not a new failure mode — just this fix's
frame-window change reaching a third case pinned by
`native_lambda_tail_capture_use_no_longer_narrows_enclosing_capture`.
