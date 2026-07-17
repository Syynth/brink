---
"@brink-lang/web": patch
---

Issue #858: `brink-fmt` now retokenizes single-line `~ expr` logic lines
through the CST instead of passing the statement's own text through
unchanged (only the outer `~ ` prefix was previously normalized). A
single-line logic line now gets the same canonical single-space-around-
tokens rendering, and the `ref lvalue-path` zero-space convention around
`.`/`[`/`]`, that a `~ { … }` multi-line block statement already received —
e.g. `~ temp x   =   0` now formats to `~ temp x = 0`, and
`~ heal(ref  party[ leader ] . hp,   5)` now formats to
`~ heal(ref party[leader].hp, 5)`. Reachable through @brink-lang/web via the
editor's "Format knot" code action (`code_actions`/`resolve_code_action`),
which runs the whole document through `brink_fmt::format`.
