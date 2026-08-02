---
"@brink-lang/web": patch
---

Fix (issue #1972, second slice): the native `.brink` surface's content-ground
`~` line escape (`stmt::logic_line`, charter §8.2) now also accepts a
`~{ … }` multi-statement logic block and a `~ until cond` condition-park
(native's sole `await` spelling — `await` itself is retired) at prose-body
position, alongside the assignment/bare-call/temp-decl shapes #1991/#1972's
first slice already wired. Both lower to the existing `Stmt::LogicBlock`/
`Stmt::Await` HIR the whole-body `~{ }` override and the ink-dialect's own
`~ { … }`/`~ await` already produce — a `~{ }` block containing only
temp-decl/assignment/call/return/`until` statements now parses, lowers, and
runs; nested `if`/`while`/`for` inside it is a narrower, emitter-only
residual (still refused, not guessed). Observable through `@brink-lang/web`
since the wasm package re-exports the native compiler/runtime pipeline.
