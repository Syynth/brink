---
"@brink-lang/web": patch
---

brink-syntax-native/brink-ir: a value-carrying `return <expr>` now parses
and lowers at content-ground (prose-body) position (issue #1973).

`parser/divert.rs::return_stmt` previously only recognized a bare `return`
or the tunnel-redirect `return -> target` at prose-body position — a
trailing value expression (`return hp > 0`) was left as dangling,
unreachable content, raising `E033`. It now parses a value expression there
too, mirroring the code-ground `return expr?;` form (`fn` bodies) that
already supported one; `lower_native::body` lowers the value into
`Stmt::Return.value`, and the `brink-respell` emitter (`emit_native.rs`)
spells it back out instead of refusing with `"return with a value
expression"`.

This is a pure grammar/lowering/emitter fix, not a semantics change: a
value-carrying `return` inside a non-function `flow` still correctly fails
with `E032` ("explicit return outside function") exactly as a bare one
would — `brink-analyzer`'s existing check is untouched. Only a `return`
inside a real `fn` newly compiles/round-trips.
