---
"@brink-lang/web": patch
---

Added hover text for the T1b stdlib slice 1 functions (`len`/`keys`/
`values`/`contains`/`push`/`insert`/`remove`, docs/t1b-surface-spec.md §5,
#589): hovering one of these names now shows its signature (mutators render
their first parameter as `name: lvalue`, e.g. `push(a: lvalue, v)`) and a
one-line semantics summary, unconditionally — like the existing built-in
(`INT`/`FLOOR`/…) hover text — so the info is available even in a strict-ink
project, where a use of the name is otherwise flagged as a brink extension.
No other wasm-exposed behavior changed: the new dialect-gated stdlib
completion, signature help, and `~ { … }` block-folding queries land in
`brink-ide` and are wired into `brink-lsp` only in this PR — the
`@brink-lang/web` bridge (`brink-web`) does not yet call them.
