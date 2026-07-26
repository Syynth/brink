---
"@brink-lang/web": patch
---

Fix (#1471): `x or default` (B1 `or`-coalescing, #1460) now **short-circuits**
— `default` is evaluated only when `x` is `none`, matching the C# `??`/Kotlin
`?:` conventions the operator's precedence placement was modeled on. The
version PR #1469 shipped evaluated both operands unconditionally (flagged
there as an unruled implementation decision); the maintainer has now ruled
short-circuiting is required, so `x or expensive()` runs `expensive()` exactly
once, and only on `none`.

The binary `Coalesce` opcode (`0xFB`) is retired — a binary opcode can't
short-circuit, since both operands would already be evaluated onto the stack
before it runs. `InfixOp::Coalesce` lowers to a real branch instead, backed by
a new opcode reusing the same byte: `CoalesceSome(rel)` pops the left-hand
`Option`, and on `some(v)` pushes the unwrapped `v` and jumps past the
right-hand operand's bytecode entirely; on `none` it falls through to evaluate
the right-hand operand as before. The collapse-vs-two-Option typing decision
(`(Option[T],T)->T` vs `(Option[T],Option[T])->Option[U]`) can no longer be
read off the right-hand operand's runtime value, so lowering consumes the
analyzer's recorded per-step types (#1492) instead and re-wraps with a
`MakeSome` after the branch only when the recorded verdict says the step keeps
its `Option`. When the left-hand type cannot be statically pinned the runtime
check remains the semantics: an `Option` coalesces, a plain value faults.

The web package's disassembly view (`program_model.rs`) and `.inkt` text
format (read + write) drop the `coalesce` mnemonic for `coalesce_some <rel>`.
Native-surface-only, so vanilla-ink and brink-dialect stories (and the oracle
corpus) are unaffected — the opcode is reachable only through native
`or`-coalescing lowering.
