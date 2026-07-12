---
"@brink-lang/web": patch
---

T1b-4 (#576): closes the indexed-write COW cliff value-model-spec §5
promises but PR #575 hadn't yet delivered — `blocks.rs`'s RMW lowering read
the root/intermediate cells it mutates via `GetGlobal`/`GetTemp`, which
`Arc`-clone the slot instead of consuming it, so `array_make_mut`/
`map_make_mut` always saw a shared `Arc` and COW-copied on every write —
O(n) per write, O(n²) for a loop of indexed writes or `push`es.

Two new opcodes in the previously-reserved sharing-discipline block
(`docs/format-v4-rfc.md` §3): `TakeGlobal(DefinitionId)` at `0xCA` and
`TakeTemp(u16)` at `0xCD` (freshly claimed, adjacent to the reservation —
`0xCB`/`0xCC` stay reserved for `StoreVarIfNew`/`EqVars`). Both move a
slot's current value out, leaving `Value::Null` behind, instead of cloning;
`TakeTemp` auto-dereferences like `GetTemp` (a `ref` parameter's pointed-to
location is taken, the pointer itself untouched).

The compiler now emits them for the **flat** RMW shape — `a[i] = v`/
`a[i] op= v` and `push`/`insert`/`remove` on a bare variable, the exact
loop-append case the spec's "one cliff" targets — with every other
sub-expression (index, value, and for indexed assignment the pre-mutation
`current` read) evaluated *before* the take, so an expression referencing
the same variable by name still sees its pre-mutation value. **Chained**
indexed assignment/mutators (`grid[y][x] = v`, `push(grid[y], v)`) are
unchanged: a nested element is still referenced from inside its parent
until the write-back cascade completes, so a take at any level but the
root buys nothing there — the sanctioned §7 clone-based fallback stays in
place for that shape.

**Fault-during-RMW slot state** (a new, deliberately-defined behavior): for
indexed assignment and `push`, a fault (out-of-bounds index, missing map
key, non-collection root) is now caught by a non-mutating pre-check
*before* anything is taken, so the root is **never** lost to a fault on
these paths — identical to the pre-#576 behavior. `insert`/`remove` at an
arbitrary author-supplied key don't get an equivalent free pre-check; a
fault there leaves the taken root holding `Value::Null` — a documented,
tested trade-off consistent with this VM's pre-existing no-rollback-on-
fault model (a fault anywhere mid-turn already leaves earlier same-turn
mutations applied).

Benchmark (`crates/brink-runtime/benches/runtime.rs`, `loop_append_bench`):
10k sequential `push`es on a freshly-created array — measured 464.8ms
median before this change, 13.91ms median after (~33x), consistent with
closing an O(n²) cliff.

Oracle corpus: unchanged, 5,577 passing episodes — T1b syntax never reaches
the strict-ink corpus by construction.
