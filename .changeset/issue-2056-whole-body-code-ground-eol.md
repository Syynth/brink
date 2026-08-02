---
"@brink-lang/web": patch
---

Fix (issue #2056): a `flow`'s whole-body `~{ }` "Compound guard" override
and a `fn`'s default code-ground body (both lowered by
`hir::lower_native::body::lower_stmt_block_as_body`) now correctly
terminate a call-containing statement run's output with a line boundary
instead of gluing it into whatever content follows. This is the same
output-boundary defect PR #2055 fixed for the single-statement
content-ground `~` escape (`lower_logic_line`'s `needs_eol` rule) — this
fix reaches the structurally distinct sibling call site
(`flush_code_ground_run`), which built `Stmt::LogicBlock` directly and
never went through `lower_logic_line`, so it never inherited that fix.
Observable through `@brink-lang/web` since the wasm package re-exports the
native compiler/runtime pipeline.
