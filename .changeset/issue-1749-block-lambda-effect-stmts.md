---
"@brink-lang/web": patch
---

Fix #1749: `InferPass::infer_lambda` now absorbs a block-bodied lambda's
own statements (`let`/assignment, not just the trailing value expression)
into the enclosing definition's effect row. Previously only the lambda's
tail expression was visited, so a block-bodied lambda's `~ temp`/assignment
statements were silently dropped from the row — a conservative-total
(`docs/effects-spec.md` §3) soundness violation. Expression-bodied lambdas
(`|x| expr`) were already sound and are unaffected. This can change
effect-row-derived diagnostics (e.g. strict-mode reads/writes/calls
checks, `@[effects(…)]` exceedance) for stories with a block-bodied lambda
whose statements (not tail) perform effects; the oracle corpus is
unaffected (no block-bodied lambda in that shape exists in the corpus
today).
