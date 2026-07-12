---
"@brink-lang/web": patch
---

T1b-2 (#570): the brink dialect now compiles and runs the full T1b surface
(docs/t1b-surface-spec.md §§2-4) — `~ { … }` logic blocks (`if`/`else if`/
`else`, `while`, `for x in arr`/`for k in map`, `break`/`continue`, `return`,
block-scoped `temp`), `#[…]`/`#{…}` sigil collection literals (constant
literals go through a new V4 literal pool, `PushLiteral(idx)`; dynamic
literals through new `ArrayNew`/`MapNew` opcodes), and postfix indexing
(`a[0]`, chained `grid[y][x]`) including indexed assignment via the ratified
RMW discipline (take → `make_mut` → write-back on the root cell; chains
lower to nested RMW through synthetic temps, never interior references).
Out-of-bounds array indices and missing map keys are turn-terminating
runtime faults on both read and write — no silent growth on write-past-end.

The `Brink` dialect no longer rejects any of this ("not yet implemented —
lands in T1b-2", `E052`) — it just compiles. `StrictInk` is unaffected
(`E051` still rejects every extension construct at its exact span).

Block-scoped `temp` declarations (including `for` loop variables) thread
into the same symbol manifest the IDE's cross-ref/rename/unused-variable
tooling reads, and get a new warning (`E054`) when they shadow an
already-visible temp (an outer classic `~ temp` or an enclosing block).

Format: a new `LiteralPool` `.inkb`/`.inkt` section (additive alongside the
existing `ListLiterals` section — `PushList` is unaffected) and twelve new
opcodes in the previously-reserved `0xBE`-`0xC9` block (`ArrayNew`, `MapNew`,
`IndexGet`, `IndexSet`, `CollectionLen`, `MapGet`, `MapInsert`, `MapRemove`,
`MapContains`, `CollectionKeys`, `CollectionValues`, `PushLiteral`) — inert
until this compiler surface emits them, so no existing `.inkb` output
changes shape unless it uses T1b syntax. Also fixes a pre-existing gap found
while adding this: the `.inkt` text format's `value`/`type_name` grammar
could not parse an `Array`/`Map` value back (only write one) — global
variable defaults with a collection default (possible since #525) now
round-trip correctly too.

Oracle corpus: unchanged, 5,577 passing episodes — the strict-ink corpus
never reaches any of this new surface by construction.
