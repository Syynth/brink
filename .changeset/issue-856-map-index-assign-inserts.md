---
"@brink-lang/web": patch
---

Indexed assignment to an absent map key now inserts (JS/Python semantics)
instead of faulting `MapKeyNotFound` — issue #856, ruled 2026-07-15.
`memo[k] = v` on a fresh key works, matching the existing `insert()`/
`push()` stdlib mutators' insert-on-absent behavior; a repeat assignment to
the same key still overwrites in place rather than growing the map.

- **`IndexSet`'s map branch** (`brink-runtime`'s `write_index_upsert`, used
  by the `IndexSet` opcode) is now insert-on-absent for a valid-domain key
  (int/string/bool) — array bounds and the map key-domain check are
  unaffected (still turn-terminating faults, no silent growth).
- **Reads are unaffected** (value-model-spec §11c): `m[k]` (`IndexGet`) and
  `MapGet` still fault `MapKeyNotFound` on a missing key. Path-projection
  writes (`ref`-bound `ProjWrite`, `docs/t1e-spec.md` §4) also keep the
  strict fault-on-missing-key behavior — only the direct `IndexSet` opcode
  changed.
- **Compiler lowering**: plain `a[idx] = v` (`lower_flat_indexed_assignment`)
  no longer runs a non-mutating pre-check read before taking the root — that
  precheck existed to catch the very fault this issue retires, and it can't
  distinguish "absent map key" from "array out of bounds" before deciding
  whether to fault. Compound assignment (`+=`/`-=`) is unaffected (the
  precheck's value is still needed as the operand). Net effect: a fault
  during plain `a[idx] = v` (array out-of-bounds, an invalid-domain map key,
  or a non-collection root) can now leave the root `Value::Null`, matching
  the documented, already-shipped trade-off `insert`/`remove`'s
  author-supplied keys make (`fault_during_insert_leaves_root_null`) —
  compound assignment still leaves the root untouched on a fault.

Observable through `@brink-lang/web`: any consumer executing compiled
`.inkb` through the wasm runtime can now see `memo[k] = v` on a fresh key
succeed instead of raising `MapKeyNotFound`, so this ships as a patch per
the wasm-observable-behavior convention. Oracle ratchet unaffected (brink-
dialect collections only — vanilla ink has none): 5,577 episodes still pass.
