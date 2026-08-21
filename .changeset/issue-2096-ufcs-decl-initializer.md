---
"@brink-lang/web": patch
---

Analyzer: UFCS resolution now reaches a decl-default lambda's own body
(issue #2096).

`ufcs::resolve` used to drive its `UfcsVisitor` with plain
`hir::visit::visit`, which never reaches a file-level `VAR`/`CONST`
initializer — so a UFCS-shaped method call written directly inside a
decl-default lambda's own body (`const callGreet = |g| g.greet(3)`, legal
since #1774's ruling) was never visited by the pass at all and fell through
to LIR lowering's defensive `E144` refusal instead of being analyzed for
real.

`ufcs::resolve` (and its `project_has_ufcs_call` laziness gate) now drives
the same `HirVisitor`-shaped visitor with
`hir::visit::visit_with_decl_initializers` — the shared entry point issue
#1571/#2098 built for exactly this class of gap — so this call site is
analyzed like any other. A receiver whose type resolves (e.g. an annotated
lambda param) now resolves and runs the desugared call for real; an
unannotated receiver with nothing else constraining its type still refuses
to compile, but now with the accurate `E142` ("annotate the receiver")
rather than the old defensive, structurally-caused `E144`.
