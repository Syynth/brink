---
"@brink-lang/web": patch
---

#1322 (B0.8 Wave B tail, `docs/decision-log.md` 2026-07-23 "Code-ground
sitting"): `brink-syntax-native`'s code-ground statement layer gains
`return e?;` (value return), `break;`/`continue;` (new hard-reserved
keywords), and compound/RMW assignment (`x += e`, `x.field += e`), plus
HIR lowering for all three. `return`'s valued form reuses the existing
`RETURN_STMT` node (content-ground `return`/`return -> x` already used
it); `break`/`continue` are brand-new node kinds with no content-ground
counterpart.

Lowers to the existing `~ { … }` T1b closed statement set
(`BlockStmt::Return`/`Break`/`Continue`, `Assignment { op: AssignOp::Add |
Sub, .. }`) — no new HIR nodes. Compound assignment mirrors the
brink-dialect's own `+=`/`-=` operator set exactly (`AssignOp` has no
`Mul`/`Div` variant on either frontend).

Reachable through any `@brink-lang/web` session that analyzes a
`.brink`-extensioned file (`brink-db`'s `lowered_query`): a `var`/`const`
initializer containing a statement-block using any of these forms (`var x
= { a += 1; return a; };`) now lowers them for real — diagnostics
included — instead of the block falling into the loud, generic E129
"unrecognized statement" arm. `#fn` function values remain unimplemented
— see `brink_ir::hir::lower_native::expr`'s module doc for the honest gap.
