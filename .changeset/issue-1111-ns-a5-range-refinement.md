---
"@brink-lang/web": patch
---

NS-A5 (#1111): ranges land as a real Value kind (F7) plus the language's
first value refinement, the inhabited range (`NonEmptyRange`). New
compileable surface in the brink dialect: `a..b` / `a..=b` range literals
(E051 under strict-ink), ranges joining the closed iterable set
(`for i in 0..n` — O(1), never materialized), `len`/indexing over ranges,
content equality (`1..=6 == 1..7`; display preserves the written form),
`pick(range)` → `Option[int]`, the `non_empty(r)` →
`Option[NonEmptyRange]` validator, and `rand::int` as the range leg of the
one value-directed `int(x)` verb (draws once, writes the RNG cell). New
wire value tag (`VAL_RANGE`, 0x11) across `.inkb`, the runtime transcript,
and `.inkt`, with a lossless `TypedValueJs::Range` on the JSON boundary
and a `{start, end, inclusive}` object on the native JS boundary. Three
new opcodes (`RangeMakeExcl`/`RangeMakeIncl`/`RangeNonEmpty`). New
compile diagnostic E117 (`types = strict` only, the E078 template):
`int(r)` demands NonEmptyRange evidence — provably-empty literals error,
provably-inhabited literals (CONST refs folded) coerce free, computed
bounds route through `non_empty`. Under gradual typing the refinement is
inert (F8's general rule) and the new turn-terminating
`EmptyRangeDraw` runtime fault is the residual. Vanilla-ink stories are
byte-identical; the oracle corpus is unaffected.
