---
"@brink-lang/web": patch
---

#1294 (B0.8 Wave A, `docs/decision-log.md` 2026-07-23 "Code-ground
sitting"): `brink-syntax-native`'s parser gains a statement layer over
its expression skeleton — `let name = expr;`, assignment `x = expr;` /
`x.field = expr;` (RMW paths), bare expression statements `expr;`, and
the `{ stmt; stmt; tail }` statement-block (an unterminated trailing
expression is the block's tail — blocks-as-values ruled, CST shape only,
no value lowering yet). A statement-block is reachable as an ordinary
expression (`var x = { let y = 1; y };` now parses with zero errors,
where it previously failed to parse the `{` at all).

Parser only — no HIR lowering. A `.brink` file that uses this new
syntax (reachable through any `@brink-lang/web` session that analyzes a
`.brink`-extensioned file, `brink-db`'s `lowered_query`) now parses
cleanly instead of surfacing a parse error, but its `STMT_BLOCK` still
reports a lowering diagnostic (E129, "not yet lowered") the same way an
unlowered `LAMBDA_EXPR` already does — never a silent drop, never a
panic. Lowering these statements is a later wave.
