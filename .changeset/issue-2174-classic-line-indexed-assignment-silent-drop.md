---
"@brink-lang/web": patch
---

Fix #2174: a classic (non-block) indexed-assignment logic line —
`~ a[i] = v`, written outside any `~ { … }` block — silently dropped the
whole statement with **zero diagnostics**: the classic-line statement
dispatch never routed an `Index` assignment target to
`lower_indexed_assignment` at all, so the write vanished. This affected
every classic-line indexed assignment, not only the struct-field-projected
root shape #2121 fixed for the `~ { … }` block surface — a bare-variable
target (`a[0] = 99`, no struct involved) compiled clean and the assignment
simply never happened.

Classic-line indexed assignment now shares the exact same dispatch the
block form already had: a bare-variable root lowers correctly, and a
struct-field-projected root (`a.items[0] = v`) is rejected with the same
non-suppressible `E074` `reject_field_projection_index_root` already
raises for the block form (#2121), instead of either silently dropping or
silently misrouting.
