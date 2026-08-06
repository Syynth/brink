---
"@brink-lang/web": patch
---

Compiler: native `fixup_return_kind` now recurses into `Stmt::LogicBlock`
(issue #1334).

`fixup_return_kind` (`brink-ir`'s native HIR lowering) walks structural
nesting to recompute `ReturnKind` after a body lowers, correcting every bare
`return` inside a non-function `flow` to `ReturnKind::TunnelRedirect` (bare
`return` there means ink's tunnel `->->`, not a function return). The
`Stmt::LogicBlock` arm — a `~{ }` code-ground body, or an `if`/`while`/`for`
nested inside one — was a no-op, so a bare `return` reached only through a
logic block kept the always-`Explicit` stamp `lower_return_stmt` gives it at
parse time, which would misfire `brink-analyzer`'s E032 ("return outside
function") for perfectly valid tunnel-return code once code-ground bodies
are reachable through this path.

Fixed by adding a parallel recursion (`fixup_return_kind_in_block_stmts`)
over `LogicBlock`'s closed `BlockStmt` set — `if`/`else if`/`else`,
`while`, `for` — applying the same non-function-bare-return correction at
every nesting depth. Off the ink pipeline; the oracle ratchet is
unaffected (5577/1027/0 episodes, 350/14/390 cases — unchanged).
