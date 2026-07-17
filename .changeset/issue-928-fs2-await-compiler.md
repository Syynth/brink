---
"@brink-lang/web": patch
---

FS-2 (#928, tracking #889): the FlowFrame compiler slice — `await`
grammar/HIR/lowering, the effect-free condition purity gate, and the LIR
lowering fence (`docs/flow-suspension-spec.md` §3/§5). Compiler + analyzer
only; the runtime spill/restore is FS-3.

New syntax reaches the wasm parser surface, so the whole grammar is
observable through `@brink-lang/web`:

- `await <cond>` parses at statement/logic position — the top-level
  `~ await …` logic line and inside a `~ { … }` block — plus the
  persistent-await `while await <cond> { … }` loop. `await` is a contextual
  (soft) keyword: it stays an ordinary assignable identifier everywhere
  else (`await = 5`, `while await { … }`), so no existing ink is affected.
- Under the default strict-ink dialect, `await` is a brink extension and is
  rejected with `E051`, like every other superset construct.
- Under `dialect = brink`, an `await` condition must be **effect-free**
  (read-only): reads are the wake dependency set, but a transitive write to
  a global cell or an effectful call is a compile error — a new diagnostic,
  `E105`, built on the effects machinery. A bare fn-value reference used as
  a dynamic condition (`await ready`) is read-only by construction and is
  never flagged.
- Every `await` construct is then fenced at LIR lowering with `E052` (the
  reserved "parses/analyzes before its lowering lands" code): its runtime
  spill/restore semantics are FS-3, so a program using `await` refuses to
  lower to bytecode rather than silently dropping the suspension point.

Vanilla ink has no `await`, so no existing story's compiled output or
runtime behavior changes.
