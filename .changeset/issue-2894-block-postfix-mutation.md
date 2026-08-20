---
"@brink-lang/web": patch
---

Compiler: `x++`/`x--` on a bare variable inside a `~ { … }` block now
actually mutates it (issue #2894).

`blocks.rs`'s `BlockStmt::ExprStmt` arm had no postfix-to-`Assign`
conversion the way `stmts.rs`'s classic-line arm does — so a bare-variable
postfix statement inside a block lowered to a pure, discarded
`lir::Expr::Postfix`: it computed `x + 1`/`x - 1` and threw the result
away, with no diagnostic and no effect. `~ { x++ }` compiled clean and
silently did nothing.

`BlockStmt::ExprStmt` now converts a bare-variable postfix operand into a
real `Assign { op: Add/Sub, value: 1 }`, mirroring the classic-line
conversion exactly. A field-operand postfix (`~ { a.count++ }`) continues
to refuse with the same non-suppressible `E074` issue #2185/PR #2897
established for the classic-line spelling, routed through the identical
`reject_field_projection_index_root` guard — this fix does not reintroduce
that misroute for the block surface.
