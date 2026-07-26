---
"@brink-lang/web": patch
---

Issue #1492 (RULED 2026-07-26, `docs/decision-log.md` "Lowering consumes
analyzer types, never re-derives"): `or`-coalescing chains are now typed by
the analyzer as a whole, and the verdict is published for LIR lowering.

The user-visible change is one widened diagnostic, brink dialect + `types =
strict` only (vanilla ink cannot produce an `or`-coalescing expression at
all, so the ink corpus is untouched):

- **E066 now judges every step of a chain, not just the innermost.**
  `{some(1) or none or "text"}` previously passed analysis silently —
  a chain's outer step had no left-hand type to check against, because
  the analyzer classified operands one node at a time and an `Expr::Infix`
  operand classifies to nothing. Each step's recorded result type is now
  fed in as the next step's left-hand type, so the mismatch is reported
  where it always was. A well-typed chain compiles exactly as before.

No runtime, bytecode, or codegen behavior changes: `Opcode::Coalesce` is
untouched, and its doc now states the ruled gradual-mode posture (with an
unpinned left-hand type, the runtime check *is* the operator's semantics —
an `Option` coalesces, a plain value faults).
