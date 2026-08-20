---
"@brink-lang/web": patch
---

Fix #2222: inside a choice's inline conditional/sequence branch (a
`lower_inline_block`-lowered `~ { … }`-less classic line), two more of
`mod.rs`'s classic-line dispatch arms are now mirrored, matching the
`Index`-assignment parity fix from #2211/#2174:

- A **struct-field assignment** (`~ p.hp = 99`) no longer silently
  corrupts the record. Before this fix it compiled with zero
  diagnostics but resolved its target to the bare root `p`, overwriting
  the whole record with the RHS and faulting at runtime
  (`RuntimeError::NotARecord`) the next time `p` was read as a struct.
  It now writes the field correctly.
- A **collection mutator call** (`~ push(a, 9)`) is no longer rejected
  with a spurious `E056` ("collection mutator used in expression
  position"). It now lowers and executes, consistent with the `~ { … }`
  block form and the top-level classic line.
