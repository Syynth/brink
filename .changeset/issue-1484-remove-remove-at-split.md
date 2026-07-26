---
"@brink-lang/web": patch
---

Issue #1484: the stdlib `remove` verb's accidental two-posture divergence
(array index removal faults out of bounds; map key removal is
idempotent-total) is fixed by renaming, not flattening. Seq index removal
is now `remove_at(a, i)`, joining the `_at` faulting-index family with
`char_at`; `remove` now uniformly names identity-based, idempotent-total
removal (map keys today; flags values once flags land). No deprecation
shim — `remove` no longer accepts an array (`NotIndexable`), and
`remove_at` no longer accepts a map. Bytecode gains one opcode
(`SeqRemoveAt`, `0xFD`) for the split primitive; wasm-observable via
`@brink-lang/web`'s bytecode disassembly view and any compiled program
using either verb.
