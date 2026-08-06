---
"@brink-lang/web": patch
---

#1972: native `.brink` source gains a content-ground temp declaration —
`~ let name: type = expr` — at the same prose-body position `~ x = expr`/
`~ expr` already used (charter §8.2's logic-line escape, extended). A
`.brink` file that previously reached for this spelling compiled with
`~ let` diagnosed as an unrecognized expression atom (`E015`-shaped, from
`expr::expression`'s fallback); it now parses, lowers, and executes as a
real temp declaration. `brink_ir::hir::emit_native` (the shared native
pretty-printer) also gained printer support for all three content-ground
statement shapes (`Stmt::TempDecl`/`Assignment`/`ExprStmt`), which were
previously refused outright even though the grammar/lowering for the
latter two already existed.

Reachable through `@brink-lang/web`: `brink-syntax-native`/
`hir::lower_native` are the same parse/lower path the wasm editor session
(`EditorSession::compile_project` and background analysis) uses for any
`.brink` document — a project authoring `~ let` prose-body statements now
compiles instead of diagnosing, and the diagnostic surface for a malformed
one changes shape.
