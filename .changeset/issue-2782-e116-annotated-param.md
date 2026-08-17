---
"@brink-lang/web": patch
---

Fix #2782: an explicitly `: Option<T>`-annotated param — an ordinary `fn`
param or a lambda's own param — now reaches the E116 `Option[T]`-condition-
truthiness check (`option_conditions.rs`). Previously only an
**inference-derived** `Option[T]` (e.g. `let r = some(3)`) was classified
there; a written annotation was silently dropped before classification,
even though `annotations::resolve` has handled `Option<T>` since #1552.

Two fix sites, one per shape:

- An ordinary `fn`/knot/stitch param: `infer::body::infer_def_body` already
  overlaid an unconstrained param's annotation onto the signature it exports
  (`InferredSig::params`), but never onto `BodyTypes::locals` — the
  bare-name-keyed map every body-level classifier (including this one)
  actually reads for a param/temp's type. Now overlaid there too, under the
  same "body wins, annotation only covers `Unknown`" firewall.
- A lambda's own param: `pruned_locals_for_lambda` pruned a lambda's own
  bindings out of the enclosing scope's locals (issue #2773's shadowing
  fix) but never seeded the lambda's own annotation back in. Now seeded
  directly, excluding any param name the lambda's own block re-binds
  (mirroring `infer_lambda`'s identical guard on its own `self.annotated`
  seed) — with a positive shadowing test confirming this doesn't reopen
  #2773's hazard.

This makes new hard E116 errors (under `types = strict`) appear on
previously-clean `.brink` files with an annotated-Option-param truthiness
condition, in both the studio Problems panel and through
`EditorSession`/`IdeSnapshot::analyze`.
