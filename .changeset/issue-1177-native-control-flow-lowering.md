---
"@brink-lang/web": patch
---

#1177 (B0.8 Wave B, `docs/decision-log.md` 2026-07-23 "Code-ground
sitting"): `brink-syntax-native`'s code-ground statement layer (B0.8 Wave
A, #1294) gains `if`/`else`/`else if`, `while`, `for name in expr`, and
`until <cond>;` control flow, plus HIR lowering for the whole statement
layer (`let`/assignment/expression statements from Wave A, and the four
new control-flow forms). `while`/`for`/`in`/`until` are new hard-reserved
keywords.

Lowers to the existing `~ { … }` T1b closed statement set
(`IfStmt`/`WhileStmt`/`ForStmt`/`AwaitStmt`) — no new HIR nodes. `until` is
native's sole condition-park spelling, lowering to the same `AwaitStmt`
node the brink-dialect's `~ await cond` produces (`await` is not a
keyword on the native surface at all).

Reachable through any `@brink-lang/web` session that analyzes a
`.brink`-extensioned file (`brink-db`'s `lowered_query`): a `var`/`const`
initializer containing a statement-block with this new control flow
(`var x = { if a { … } };`) now lowers its control-flow statements for
real — diagnostics included — instead of the block's contents being
silently unlowered. The block's own *value* (blocks-as-values) still has
no HIR representation, so the outer `STMT_BLOCK` still reports its own
E129 the same way an unlowered `LAMBDA_EXPR` does — never a silent drop,
never a panic. Wiring this statement layer into `flow`/`fn` declaration
bodies (replacing the content-ground `BLOCK` those still use) is a later,
separate slice (#1309).
