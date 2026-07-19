# Phase C — the stdlib inventory tables (DRAFT)

Drafted 2026-07-19 from the fully-ruled `docs/stdlib-spec.md` §§1–9
(branch `origin/docs/stdlib-spec-domains-4-7`), the native-surface
charter §§11–13, the effects/T2 spec, the typed-mode spec, issues
#1087/#1097/#1103, and every 2026-07-18 decision-log entry.

**This is a composition draft, not a ruling.** Where a cell could not
be filled from ruled text it is marked `⚠Fn` pointing at the numbered
finding in `phase-c-findings.md`. Blocking findings mean the marked
cells are provisional. Findings ruled 2026-07-19 have been folded in:
their `⚠Fn` markers are replaced with the ruling one-liners (see
`docs/decision-log.md`, 2026-07-19 entries); remaining `⚠Fn` are
still open.

> **As-built status (2026-07-19, `origin/main` @ `f2ee96eb`).** This
> inventory now tracks shipped-vs-specified reality through the NS
> substrate waves **A1 (Option, PR #1118) · A2 (effect rows, PR #1121)
> · A3 (protocol registry, PR #1124) · A6 (rng-as-cell, PR #1125)**;
> A4/A5/A7/A8 and all of Track B are **not yet built**. §13's old
> "≈93 shipped" counted the *ruled* surface — the actually-dispatched
> intrinsic set on main is **29 call names** (verified against both
> `is_t1b_stdlib_name` copies, `brink-analyzer/src/resolve.rs` +
> `brink-ir/src/lir/lower/expr.rs`, and `infer_intrinsic`'s arms):
> `len keys values contains push insert remove int float string call
> bind char_at find index_of min max first last pop get contains_value
> clear some chance pick shuffle shuffled seed`, plus variable-position
> `none`. **Built column legend** (added per verb row below): ✅ =
> dispatched on main · 🔜A4/A5/A7/A8/B_n_ = ruled, pinned to that wave
> (`docs/stdlib-sequencing.md`) · 🔜 = ruled, not yet sequenced into a
> wave.
>
> **Allocated diagnostics**: E107 (bare `none` needs a type — A1) ·
> E108/E109 (`@[effects(silent)]`/`(total)` exceedance — A2) ·
> E110/E111/E112 (annotation surface — A2) · E113/E114/E115 (reserved
> protocol names / contract exceedance / ill-formed impl registration
> — A3) · E120 (`weighted` construction refusal, the owed §8 NEW code
> — A7). **Allocated opcodes**: 0xDE/0xDF (`PushNone`/`MakeSome`),
> 0xE2–0xEB (NS-A1 verb flips), 0xEC–0xEF (NS-A6 rand draws; `seed`
> reuses frozen `SeedRandom` 0x85), 0xFA (`Collect` — NS-A7 weighted +
> heap family, one opcode + kind byte per the 0xF7 `Tower` economy).
>
> **As-built caveats**: (1) A1 shipped falsy-`none` truthiness; the
> queue's **F27 ruling (2026-07-19) supersedes it** — Option has NO
> truthiness (condition-position Option = strict compile error /
> gradual fault); implementation fix owed, rides #1120. (Queue
> F27/F28 — truthiness / pre-B4 total `none`/`some(…)` display — are
> distinct from `phase-c-findings.md`'s F27, the concat naming-axis
> query, which stays open.) (2) A2's `faults` row is bool-conservative;
> the protocol-contract granularity question is **F29** (audit C1+C2),
> gating A4.

---

## 0. Reading the tables

### 0.1 Display notation (RULED §9.4)

> **Display notation banner — standing:** signatures use the
> pseudo-generic letter form (`T`, `U`, `K`, `V`, `I`). **`T` is not
> writable in source** (#1090 guards the user-generics door). The
> notation exists because UFCS completion already renders this shape;
> docs and IDE agree. `[T]` = array, `[K: V]` = map, `Option[T]`,
> `Weighted[T]` are the four compiler-known parameterized builtins
> (arrays, maps, Option, Weighted). `iterable[T]` = the closed
> iterable set (arrays · ranges · flags subsets · maps-by-keys),
> not a source-writable type.

### 0.2 Effect-row column (reads/writes/emits/tags/faults)

Every verb row records the full T2 row extended by #1087/#1097:
**reads · writes · emits · tags · faults**. Compact encoding:

| Token | Meaning |
|---|---|
| `P` | pure·silent·total — reads ∅, writes ∅, emits no, tags no, faults no |
| `W:rng` | writes the `std::rand` RNG state cell (§7; no new row dimension — an ordinary write) |
| `F:oob` | faults on out-of-bounds index (the indexing contract; a computed-wrong index is a bug) |
| `F:parse` | faults on malformed input (E078 lineage) |
| `F:float` | **unconditional** `faults` over `[float]` — the mode-independent conservative union (§4b); prod never fires it, the checker doesn't know modes |
| `F:malformed` | faults on a malformed *question* (multi/empty-subset `next`/`index_of`) |
| `F:div0` | faults on zero divisor (⚠F23) |
| `⊕f` | **composes the callback's row** (#872): the verb reads what `f` reads, faults iff `f` faults, etc. Pure-required callbacks (the trio) contribute only reads+faults; effectful callbacks (`each`/`map_each`) contribute writes+emits+tags too |

`R:∅` is elided (all non-`⊕` stdlib verbs read no world cells).

### 0.3 Posture columns

- **Option/refine** — does the verb return `Option[T]` (typed
  absence) or consume/produce a value refinement (inhabited range)?
- **Proto** — protocol dependency: `display` (§9.6), `compare`
  (ordering), `iterate` (pull `next`), or `eq` (built-in structural
  equality, *not* a registry protocol — §9.6 has no equality entry).
- **NS** — namespace. **Pre** — in the ambient prelude (✓) or not (·).
- **Recv** — UFCS receiver: `lval` (auto-refs an lvalue — mutating),
  `val` (any value — functional/query), `—` (no natural receiver, e.g.
  `all(Mood)`, `rand::float()`).

---

## 1. Domain 1 — math scalar kit (§2)

Prelude: **entire kit incl. trig** (generous ruling). NS `std::math`
(prelude-granted). All int/float via checker-known intrinsic overloads
(the doctrine's first exercise). NaN-totality: `sqrt(-1.0)` = NaN,
never a fault (§2). No protocol deps.

**Built: none.** No scalar-math verb is dispatched on main, and the kit
is not pinned to any Track A wave (`docs/stdlib-sequencing.md` sequences
A1–A8 without it). The dispatched `min`/`max` on main are §4's
array-extremum forms (`[T] → Option[T]`) — the two-arg scalar overloads
are not built.

| Verb | Signature | NS | Pre | Recv | Row | Option/refine |
|---|---|---|---|---|---|---|
| `abs` | `fn abs(x: float): float` (int overload) | math | ✓ | val | P | — |
| `sign` | `fn sign(x: float): float` (int overload) | math | ✓ | val | P | — |
| `min` | `fn min(a: float, b: float): float` (int, tower) | math | ✓ | val | P | — ⚠F11 |
| `max` | `fn max(a: float, b: float): float` (int, tower) | math | ✓ | val | P | — ⚠F11 |
| `clamp` | `fn clamp(x: float, lo: float, hi: float): float` | math | ✓ | val | P | — |
| `lerp` | `fn lerp(a: float, b: float, t: float): float` | math | ✓ | val | P | — |
| `inverse_lerp` | `fn inverse_lerp(a: float, b: float, v: float): float` | math | ✓ | val | P | — |
| `smoothstep` | `fn smoothstep(edge0: float, edge1: float, x: float): float` | math | ✓ | val | P | — |
| `floor` | `fn floor(x: float): float` | math | ✓ | val | P | — |
| `ceil` | `fn ceil(x: float): float` | math | ✓ | val | P | — |
| `round` | `fn round(x: float): float` | math | ✓ | val | P | — |
| `trunc` | `fn trunc(x: float): float` | math | ✓ | val | P | — |
| `sqrt` | `fn sqrt(x: float): float` | math | ✓ | val | P (NaN-total) | — |
| `pow` | `fn pow(base: float, exp: float): float` | math | ✓ | val | P | — |
| `sin` | `fn sin(x: float): float` | math | ✓ | val | P | — |
| `cos` | `fn cos(x: float): float` | math | ✓ | val | P | — |
| `tan` | `fn tan(x: float): float` | math | ✓ | val | P | — |
| `atan2` | `fn atan2(y: float, x: float): float` | math | ✓ | val | P | — |
| `div_floor` | `fn div_floor(a: int, b: int): int` | math | ✓ | val | `F:div0` ⚠F23 | — |
| `mod_floor` | `fn mod_floor(a: int, b: int): int` | math | ✓ | val | `F:div0` ⚠F23 | — |

Constants: `const PI: float`, `const TAU: float` (prelude).

**Frozen siblings**: heritage `/` and `%` stay **truncating operators**
(ink-frozen, oracle-guarded) — `div_floor`/`mod_floor` are the blessed
grid verbs, a two-surface pair at the operator level.

---

## 2. Domain 1b — the numeric tower (§2b)

Closed compiler-known tower: **vec2 / vec3 / vec4 / quat** + matrices
**mat2/mat3/mat4** (all sizes v1). f32 components (glam-aligned). Global
type names like `int`; verbs live in `std::math`. **No user operator
overloading** — the closed tower is the middle ground. **F24 RULED
(2026-07-19, airport sitting)**: the mini-spec landed —
`docs/tower-mini-spec.md`: glam-backed Value kinds, all matrix sizes,
lane-serialized little-endian f32 wire (never glam memory/serde),
componentwise-IEEE equality, tower **not orderable**; conventions per
glam wholesale (decision-log 2026-07-19).

**Built: none — the whole domain is wave A8** (#1114, unblocked by the
mini-spec; no tower dispatch entries on main).

| Verb / op | Signature / rule | NS | Pre | Row | Notes |
|---|---|---|---|---|---|
| `dot` | `fn dot(a: vecN, b: vecN): float` | math | ✓* | P | vec2/3/4 |
| `cross` | `fn cross(a: vec3, b: vec3): vec3` | math | ✓* | P | vec3 only |
| `+` `-` | componentwise | (op) | — | P | vecN, quat |
| `*` scalar | `vecN * float`, `float * vecN` scale | (op) | — | P | |
| `mat * vec` | transform | (op) | — | P | mat2/3/4 (all sizes v1 — mini-spec) |
| `quat * quat` | compose | (op) | — | P | |
| `quat * vec` | rotate | (op) | — | P | |
| `min`/`max`/`clamp`/`lerp` | componentwise across the tower (width-1 scalar floor is §1) | math | ✓ | P | "defined once across it" (§2b) |

\* prelude flag for tower verbs inherits the "entire math kit" prelude
ruling; confirm `dot`/`cross` are intended prelude entries with the
A8 build.

Construction: `vec3 { x: 1.0, y: 2.0, z: 3.0 }` (grammar §K); equality
componentwise-IEEE, display/save per the mini-spec (F24 ruled).

---

## 3. Domain 2 — text (§3)

Characters = Unicode scalar values (USV); graphemes out of core.
Casing = locale-independent Unicode simple mapping. Prelude:
`len contains char_at`; rest `std::text`. `slice`/`trim`/`split` return
**Views** (§3b — a representation, not a type; row is `P`, view-ness is
unobservable). No protocol deps except structural `eq` for `contains`.

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | Proto | Built |
|---|---|---|---|---|---|---|---|---|
| `len` | `fn len(s: string): int` | text | ✓ | val | P | — | — | ✅ |
| `char_at` | `fn char_at(s: string, i: int): string` | text | ✓ | val | `F:oob` | — | — | ✅ |
| `slice` | `fn slice(s: string, start: int, end: int): string` | text | · | val | `F:oob` ⚠F13 | — (View) | — | 🔜 |
| `contains` | `fn contains(s: string, sub: string): bool` | text | ✓ | val | P | — | eq | ✅ |
| `starts_with` | `fn starts_with(s: string, prefix: string): bool` | text | · | val | P | — | — | 🔜 |
| `ends_with` | `fn ends_with(s: string, suffix: string): bool` | text | · | val | P | — | — | 🔜 |
| `find` | `fn find(s: string, sub: string): Option[int]` | text | · | val | P | **Option[int]** (martyr #1) | — | ✅ (0xE2) |
| `replace` | `fn replace(s: string, from: string, to: string): string` | text | · | val | P | — | — | 🔜 |
| `split` | `fn split(s: string, sep: string): [string]` | text | · | val | P | — (Views) | — | 🔜 |
| `join` | `fn join(parts: [string], sep: string): string` | text | · | val | P | — | — | 🔜 |
| `trim` | `fn trim(s: string): string` | text | · | val | P | — (View) | — | 🔜 |
| `repeat` | `fn repeat(s: string, n: int): string` | text | · | val | P ⚠F25 (n<0?) | — | — | 🔜 |
| `upper` | `fn upper(s: string): string` | text | · | val | P | — | — | 🔜 |
| `lower` | `fn lower(s: string): string` | text | · | val | P | — | — | 🔜 |

The 🔜 text verbs are not pinned to a Track A wave (unsequenced).

---

## 4. Domain 3 — sequences (§4, §4b)

Type `[T]`, literal `[…]`; the word **"list" retires entirely**.
Prelude: `len contains push`; rest `std::seq`. Mutation posture:
imperative = in-place (`ref` first param, `void` return), past-participle
= functional (returns new). UFCS auto-refs lvalue receivers; **rvalue
receiver on a mutating verb = compile error**.

**Ordering doctrine (§4b)**: `sort`/`sorted`/`min`/`max` over `[float]`
carry `F:float` unconditionally (dev-fault on NaN / prod pinned order);
over `[int]`/`[string]`/`[bool]` they are total. Structs/enums order
only via a registry `compare` impl.

**As-built ordering roster (audit C9)**: A1's `total_order_cmp`
(`brink-runtime/src/collection_ops.rs`) orders **scalar elements only**
(int/float/bool/string) — arrays-of-arrays (lexicographic) and
structs/enums both fault `NotOrderable` today. The full roster (array
lexicographic recursion, `compare`-protocol structs) and the dev/prod
NaN split land with **A4**; until then the shipped `min`/`max` are
scalar-element verbs.

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | Proto | Built |
|---|---|---|---|---|---|---|---|---|
| `len` | `fn len(a: [T]): int` | seq | ✓ | val | P | — | — | ✅ |
| `first` | `fn first(a: [T]): Option[T]` | seq | · | val | P | **Option[T]** (empty→none) | — | ✅ (0xE6) |
| `last` | `fn last(a: [T]): Option[T]` | seq | · | val | P | **Option[T]** | — | ✅ (0xE7) |
| `index_of` | `fn index_of(a: [T], x: T): Option[int]` | seq | · | val | P | **Option[int]** (martyr #2) | eq ⚠F21 | ✅ (0xE3) |
| `contains` | `fn contains(a: [T], x: T): bool` | seq | ✓ | val | P | — | eq ⚠F22 | ✅ |
| `slice` | `fn slice(a: [T], start: int, end: int): [T]` | seq | · | val | `F:oob` ⚠F13 | — (View) | — | 🔜 |
| `concat` | `fn concat(a: [T], b: [T]): [T]` | seq | · | val | P | — | — ⚠F27 (findings-doc numbering: naming-axis, open) | 🔜 |
| `sort` | `fn sort(ref a: [T]): void` | seq | · | lval | `F:float` | — | compare | 🔜A4 |
| `sort_by` | `fn sort_by(ref a: [T], cmp: fn(T,T): int): void` (F0 RULED 2026-07-19: in-place, `ref a`, per the naming law) | seq | · | lval | `⊕cmp` + inconsistency-fault ⚠F14 | — | — | 🔜A4 |
| `sorted` | `fn sorted(a: [T]): [T]` | seq | · | val | `F:float` | — | compare | 🔜A4 |
| `sorted_by` | `fn sorted_by(a: [T], cmp: fn(T,T): int): [T]` (added by the F0 ruling — the functional past-participle twin) | seq | · | val | `⊕cmp` + inconsistency-fault ⚠F14 | — | — | 🔜A4 |
| `reverse` | `fn reverse(ref a: [T]): void` ⚠F26 | seq | · | lval | P | — | — | 🔜 |
| `reversed` | `fn reversed(a: [T]): [T]` | seq | · | val | P | — | — | 🔜 |
| `min` | `fn min(a: [T]): Option[T]` | seq | · | val | `F:float` | **Option[T]** ⚠F2,F11 | compare | ✅ (0xE4; scalar elements only until A4 — see C9 note above) |
| `max` | `fn max(a: [T]): Option[T]` | seq | · | val | `F:float` | **Option[T]** ⚠F2,F11 | compare | ✅ (0xE5; scalar elements only until A4) |
| `push` | `fn push(ref a: [T], x: T): void` | seq | ✓ | lval | P | — | — | ✅ |
| `pop` | `fn pop(ref a: [T]): Option[T]` | seq | · | lval | P | **Option[T]** (empty→none) | — | ✅ (0xE8) |
| `insert` | `fn insert(ref a: [T], i: int, x: T): void` | seq | · | lval | `F:oob` | — | — | ✅ |
| `remove` | `fn remove(ref a: [T], i: int): ???` **⚠F5b** | seq | · | lval | `F:oob` | — | — | ✅ |
| `each` | `fn each(a: [T], f: fn(T): void): void` | seq | · | val | `⊕f` (writes/emits/tags/faults all compose) | — | — | 🔜 |
| `map` | `fn map(a: [T], f: fn(T): U): [U]` | seq | · | val | `⊕f` (reads+faults only; f pure-required) | — | — | 🔜 |
| `filter` | `fn filter(a: [T], pred: fn(T): bool): [T]` | seq | · | val | `⊕pred` (pure-required) | — | — | 🔜 |
| `fold` | `fn fold(a: [T], init: U, f: fn(U,T): U): U` | seq | · | val | `⊕f` (pure-required) | — | — | 🔜 |
| `filter_map` | `fn filter_map(a: [T], f: fn(T): Option[U]): [U]` | seq | · | val | `⊕f` (pure-required) ⚠F9 | callback returns Option | — | 🔜 |
| `map_each` | `fn map_each(a: [T], f: fn(T): U): [U]` | seq | · | val | `⊕f` (full: writes/emits/tags; sequential, never fused) | — | — | 🔜 |

Unmarked-🔜 seq verbs (slice/concat/reverse/reversed + the trio and its
effectful spellings) are not pinned to a Track A wave (unsequenced).

**🔶 `for ref m in maps { m[k] = v }`** — mutating iteration (loop form,
not a verb; index-desugar over RMW; #829 projections stay icebox).

---

## 5. Domain 4 — maps (§5)

Type `[K: V]`, literal `Map { k: v }`. Keys = scalar map-key set + unit
enum variants (non-scalar key = E076 lineage). Prelude: `len` only; rest
`std::map`. `m[k]` read-faults, `m[k] = v` inserts (operators, #856).

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | Proto | Built |
|---|---|---|---|---|---|---|---|---|
| `len` | `fn len(m: [K: V]): int` | map | ✓ | val | P | — | — | ✅ |
| `contains_key` | `fn contains_key(m: [K: V], k: K): bool` | map | · | val | P | — | — | 🔜 (not dispatched — note: map membership on main is the polymorphic `contains`) |
| `contains_value` | `fn contains_value(m: [K: V], v: V): bool` | map | · | val | P (O(n)) | — | eq | ✅ (0xEA) |
| `get` | `fn get(m: [K: V], k: K): Option[V]` | map | · | val | P | **Option[V]** (martyr #3) | — | ✅ (0xE9) |
| `keys` | `fn keys(m: [K: V]): [K]` | map | · | val | P (insertion-order snapshot) | — | — | ✅ |
| `values` | `fn values(m: [K: V]): [V]` | map | · | val | P (insertion-order snapshot) | — | — | ✅ |
| `remove` | `fn remove(ref m: [K: V], k: K): void` | map | · | lval | P (idempotent-total) | — | — | ✅ |
| `clear` | `fn clear(ref m: [K: V]): void` | map | · | lval | P | — | — | ✅ (0xEB) |
| `insert` | **RESERVED — not shipped as a map verb** (write-index is insertion; verb-form demand = #1103/code-dialect exhibit #1; the dispatched `insert` on main is §4's array positional insert) | map | — | — | — | — | — | — |
| *(operator)* `m[k]` | read | — | — | val | `F:oob`(missing key) | — | — | ✅ |
| *(operator)* `m[k] = v` | insert-or-update | — | — | lval | P | — | — | ✅ |

Pair iteration: **`for k, v in m`** — desugars to key-iteration +
`let v = m[k]`. **F10 RULED (2026-07-19)**: the key set is
**snapshotted eagerly at loop entry** — maps' `for` is a deliberate
exception to live pull iteration; structural modification mid-loop sees
the snapshot's keys, and a snapshotted key since removed hits the
faulting read honestly (decision-log 2026-07-19; surface lands with B2).
No `entries` verb; `keys`/`values` reify the two projections.

---

## 6. Domain 5 — flags (§6) — the LIST-op audit

Post-rename surface for `flags` (ordered domain of named symbols,
subset-valued variables). `Mood` below stands for any flags type.
Prelude: `contains count`; rest `std::flags`. Two-surface: the frozen
ink LIST ops keep total-empty semantics; the ✚ native verbs return
`Option` on absence.

**Built: none of the native verbs.** No flags verb is dispatched on
main, and the domain is not pinned to a Track A wave (unsequenced;
operator forms `?`/`^`/`+=`/`-=` are code-dialect ⏳). The frozen ink
LIST ops (right column) remain the only shipped surface — plus
`rand::pick` over a flags subset, which A6 did ship (§7).

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | ink frozen sibling |
|---|---|---|---|---|---|---|---|
| `count` | `fn count(s: Mood): int` | flags | ✓ | val | P | — | `LIST_COUNT` |
| `all` | `fn all(Mood): Mood` | flags | · | — | P | — | `LIST_ALL` |
| `none` | `fn none(Mood): Mood` | flags | · | — | P | — | `()` empty |
| `contains` | `fn contains(s: Mood, m: Mood): bool` | flags | ✓ | val | P | — | `?` |
| `add` | `fn add(ref s: Mood, m: Mood): void` | flags | · | lval | P (idempotent) | — | `+=` |
| `remove` | `fn remove(ref s: Mood, m: Mood): void` | flags | · | lval | P (idempotent-total) | — | `-=` |
| `intersect` | `fn intersect(a: Mood, b: Mood): Mood` | flags | · | val | P | — | `^` |
| `first` | `fn first(s: Mood): Option[Mood]` | flags | · | val | P | **Option** (empty→none) | `LIST_MIN` |
| `last` | `fn last(s: Mood): Option[Mood]` | flags | · | val | P | **Option** | `LIST_MAX` |
| `index_of` | `fn index_of(m: Mood): int` | flags | · | val | `F:malformed` | int; multi/empty faults ⚠F21 | `LIST_VALUE` (conversion frozen) |
| `range` | `fn range(Mood, a: Mood, b: Mood): Mood` | flags | · | — | P | — | `LIST_RANGE` |
| `invert` | `fn invert(s: Mood): Mood` | flags | · | val | P | — | `LIST_INVERT` |
| `next` | `fn next(s: Mood): Option[Mood]` | flags | · | val | `F:malformed` | **Option** (edge→none; multi/empty faults) | `+1` (frozen) |
| `prev` | `fn prev(s: Mood): Option[Mood]` | flags | · | val | `F:malformed` | **Option** | `-1` (frozen) |

`rand::pick` accepts a flags subset (§7). The numeric coupling
(explicit flag values, subsets↔ints) stays **frozen** — never respelled.
Operator forms (`?`, `^`, `+=`/`-=`) ⏳ code-dialect sitting.

---

## 7. Domain 6 — random (§7)

NS `std::rand`, **no prelude entries** (draws are deliberate acts). RNG
is a named state cell; every draw is an ordinary `W:rng` write — **no
new effect dimension**. Determinism: algorithm pinned, state saves/loads
with the story, seeded replay identical cross-platform.

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | Proto | Built |
|---|---|---|---|---|---|---|---|---|
| `int` | `fn int(r: <inhabited range>): int` | rand | · | — | `W:rng` | **consumes inhabited-range refinement** (total by type) | — | 🔜A5 (range-draw form; the `int(x)` *conversion* is shipped, §10) |
| `float` | `fn float(): float` → [0,1) | rand | · | — | `W:rng` | — (F4 resolved in-wave A6: nullary = draw, unary = the conversion) | — | ✅ (0xEC) |
| `chance` | `fn chance(p: float): bool` | rand | · | — | `W:rng` (F3 RULED 2026-07-19: `p` clamps to [0,1], NaN → false — total; interpretation, not fabrication) | — | — | ✅ (0xED) |
| `pick` | `fn pick(it: iterable[T]): Option[T]` | rand | · | lval/val | `W:rng` | **Option[T]** (empty→none) ⚠F18 | iterate | ✅ (0xEE; arrays + flags subsets) |
| `shuffle` | `fn shuffle(ref a: [T]): void` | rand | · | lval | `W:rng` | — | — | ✅ (0xEF) |
| `shuffled` | `fn shuffled(a: [T]): [T]` | rand | · | val | `W:rng` | — | — | ✅ |
| `seed` | `fn seed(n: int): void` | rand | · | — | `W:rng` | — | — | ✅ (reuses frozen `SeedRandom` 0x85) |
| `roll` | `fn roll(w: Weighted[T]): T` | rand | · | val | `W:rng` | — (total by construction — §8) | — | ✅ (`Collect` 0xFA) |
| `nonempty` | `fn nonempty(r: range): Option[<inhabited range>]` | rand? (F7 RULED 2026-07-19: ranges are a REAL Value kind — wire/equality/display/save; A5 specifies, incl. the NS home) | · | val | P | **produces the refinement** (validator) | — | 🔜A5 |

**Frozen siblings**: ink `RANDOM(min, max)` / `SEED_RANDOM` are frozen
spellings of the same cell — one RNG, two surfaces, no drift.

---

## 8. Domain 7 — collections+ (§8)

NS `std::collections`. Heap = verbs over ordinary `[T]` (min-heap; zero
new value kinds). `Weighted[T]` is a parameterized builtin.

**Built: the whole domain shipped in wave A7** (issue #1113): one
`Collect` opcode (0xFA, kind byte — the `Tower` economy), VAL_WEIGHTED
wire tag 0x19, `Ty::Weighted` in the checker. Brink-dialect spelling of
the chartered literal until B5: `weighted(w1, v1, w2, v2, …)`.

| Verb / type | Signature | NS | Pre | Recv | Row | Option/refine | Proto | Built |
|---|---|---|---|---|---|---|---|---|
| `heap_push` | `fn heap_push(ref a: [T], x: T): void` | collections | · | lval | `F:float` (NaN entry-check §4b) | — | compare | ✅ A7 |
| `heap_pop` | `fn heap_pop(ref a: [T]): Option[T]` | collections | · | lval | P | **Option[T]** (empty→none) | compare | ✅ A7 |
| `heap_peek` | `fn heap_peek(a: [T]): Option[T]` | collections | · | val | P | **Option[T]** | compare | ✅ A7 |
| `Weighted[T]` | type; literal `Weighted { weight: value }` | collections | — | — | — (construction refuses empty/zero/neg — evidence-by-construction; **E120** where classifiable, construction fault for computed weights) | evidence refinement; F17 multiset equality (RULED, as built) | — | ✅ A7 |
| `roll` | (in `std::rand`, §7) | rand | · | val | `W:rng` | total by construction | — | ✅ A7 |

`while heap_pop(ref open) as node { … }` is the drain loop — depends on
the `as`-binding construct ⚠F16. `len`/iteration/mutation for `Weighted`
and heap deferred (⏳ v1 = construct-and-roll / push-pop-peek).

---

## 9. The registry protocols (§9.6) — contracts & laws

A **CLOSED** set of compiler-declared protocols user types (structs,
enums) may **implement but never declare**. No bounds, no user
generics, no user-defined protocols. Two-tier: closed overload families
(math kit, tower, `len`/`contains`) stay mechanism-free intrinsics;
registry entries exist only where user types participate in a compiler
behavior. Promotion evidence-gated via #1090.

| Protocol | Signature | Effect contract (checker-enforced on impls) | Default | Laws |
|---|---|---|---|---|
| `display` | `fn display(self: T): string` | row ⊆ **pure·silent·total** | structural default for enums/structs; user impl overrides; machine states inherit (#905) | feeds the §1.6 display boundary; F1 RULED 2026-07-19: BOTH interpolation and `string()` dispatch through `display` — one display path; F6 RULED 2026-07-19: `display`/`compare`/`next` are RESERVED names — author shadowing is a hard error (E113), not E035 |
| `compare` | `fn compare(a: T, b: T): int` | row ⊆ **pure·silent·total** | none (no structural auto-order; derive-by-fields evidence-gated) | slots into the §4b ordering doctrine; coherence RULED 2026-07-19 (F15 closed): `compare` is ORDERING ONLY — equality stays structural always; `compare == 0` need not imply `==`, divergence legal and documented (sort never implies dedup) |
| `iterate` | `next(ref Self): Option[T]` | row ⊆ **writes-receiver·silent·total** | machine-form impls make laws structural | "every element once; `none` is terminal and sticky" — property-harness enforced |

**Why pull-shaped iterate** (RULED): a push-desugared `for` body is an
fn-value callback and **functions never await** — push would ban `await`
inside `for` bodies in flows; pull desugars inline and iterators park
across suspensions. `for` is the only v1 consumer (concrete-site
resolution under mono-HM, zero generics); user iterables joining
`map`/`filter`/`fold` stays #1090-gated. `each`/`for_each` remain derived
verbs for pure-callback cases. Implementation spelling (attribute vs
impl-block) ⏳ code-dialect sitting.

Note: **structural equality (`eq`) is NOT a registry protocol** — it is
the built-in content-comparison ruled 2026-07-18 (insertion-order-
insensitive for maps/records). `contains`/`index_of`/`contains_value`
depend on it, not on `compare`. This asymmetry (equality built-in,
ordering protocol-gated) is now ruled doctrine (the F15/coherence
ruling, 2026-07-19): equality stays structural forever; `compare` is
ordering-only.

**As-built (A3, PR #1124)**: the registry *machinery* is on main —
reserved-name enforcement (E113), the per-protocol effect-contract gate
(E114), registration validation (E115), structural display defaults —
but **no `.ink` impl spelling exists yet** (⏳ code-dialect sitting).
The contract's `faults`-granularity question (audit C1/C2 → **F29**)
gates A4.

---

## 10. The two-surface table (frozen-ink ↔ native)

The `int()`/`INT()` precedent generalized: one runtime op, two source
surfaces, oracle byte-identical. The frozen surface keeps ink's legacy
semantics untouched; the native surface carries the ruled postures.

| Native (posture) | Frozen ink sibling | Divergence |
|---|---|---|
| `int(x)` / `float(x)` (fault on parse-fail, E078) | `INT()` / `FLOAT()` (silent-0-on-fail) | fault vs silent-zero |
| `string(x)` (total, display form) | (interpolation `{x}`) | same display form (F1 RULED 2026-07-19: one display path — both dispatch through the `display` protocol) |
| seq/flags `first`/`last` → `Option` | `LIST_MIN` / `LIST_MAX` (total, empty semantics) | Option vs silent-empty |
| flags `count` | `LIST_COUNT` | rename only |
| flags `all`/`none`/`range`/`invert` | `LIST_ALL`/`()`/`LIST_RANGE`/`LIST_INVERT` | rename only |
| flags `index_of` → int | `LIST_VALUE` (subsets↔ints conversion) | native = ordinal query; the numeric conversion stays **frozen** |
| flags `contains` | `?` | verb vs operator |
| flags `add`/`remove` | `+=` / `-=` | verb + operator both kept |
| flags `intersect` | `^` | verb; operator ⏳ |
| flags `next`/`prev` → `Option` | `+1` / `-1` stepping | Option-at-edge vs frozen arithmetic |
| `rand::int(range)` (total by refinement) | `RANDOM(min, max)` | refinement vs runtime-checked |
| `rand::seed(n)` | `SEED_RANDOM` | rename only |
| `rand::pick(flags subset)` → `Option` | `LIST_RANDOM` | Option vs frozen op |
| `div_floor`/`mod_floor` | `/` / `%` (truncating operators) | floor vs truncate |

---

## 11. Construction forms — `TypeName { … }` grammar (§9.1, #1103)

One initializer grammar, **per-type meaning** (protocol-vs-grammar is
#1103, code-dialect sitting; this sitting commits only to the shape).

| Type | Form | Inside the braces | Duplicate policy |
|---|---|---|---|
| struct | `Point { x: 1.0, y: 2.0 }` | fields (+defaults); source-order eval | duplicate field = compile error E084 |
| map | `Map { k: v }` | key: value pairs | duplicate key = E076/E084 lineage (error) |
| flags | `Flags { calm, wary }` / `none(Mood)` / `all(Mood)` | bare members (a subset) | dup member = ⚠(idempotent? or error) |
| array | `[a, b, c]` | elements | n/a |
| `Weighted[T]` | `Weighted { 3: sword, 1: shield }` | weight: value pairs, positive-int weights | **duplicate weight LEGAL (multiset)** ⚠F17 — diverges from map! |
| enum variant | `Phase.Suspicious { level: 0.5 }` | named-field payload | duplicate field = E084 |
| tower | `vec3 { x:, y:, z: }` | components per `docs/tower-mini-spec.md` (F24 ruled) | — |

Struct pattern in `let` reuses match's grammar (§1.3).

---

## 12. Value refinements (§7 doctrine)

CLOSED, compiler-known refinements only — the checker mints the
evidence; **no user-written predicates** (liquid-types = own future
ledger; population today: **two**). **Built: neither** — inhabited
range lands with A5 (on the F7-ruled range Value kind),
Weighted-by-construction with A7.

| Refinement | Introduced by | Construction | Consumers | Gradual mode |
|---|---|---|---|---|
| **inhabited range** | `1..=6` literal (const-folded bounds coerce free); `(a..b).nonempty()` for computed bounds | statically-empty literal (`0..0`) = compile error; computed → `Option[<inhabited range>]` | `rand::int` (total by type) | F8 RULED 2026-07-19: refinements are INERT in gradual with a runtime-fault residual — `rand::int` faults on an empty range under gradual (the `int()`/E078 precedent; recorded as the general rule for all future refinements) |
| **Weighted-by-construction** | `Weighted { … }` literal (`weighted(w, v, …)` in the brink dialect until B5) | empty/zero/negative refused: compile error **E120** where classifiable (literal weights, empty/odd pair rows), **construction fault** (`WeightedBadWeight`) for computed weights | `rand::roll` (total by construction) | construction fault carries to gradual (E078-style split); E120 fires in BOTH regimes (it lives at the lowering, not the checker) |

Recorded evolution (not built): a validating `Option`-returning
constructor verb for `Weighted` if dynamic table-building shows demand —
the way `nonempty()` killed the range construction-fault residual.

---

## 13. Verb count — ruled vs built

| Domain | Verbs (ruled) | Built on main | Notes |
|---|---|---|---|
| math scalar (§1) | 20 | 0 | + 2 constants (PI, TAU); `/`,`%` operators frozen; unsequenced |
| numeric tower (§2b) | 2 verbs (`dot`,`cross`) + operators + 4 tower-wide (min/max/clamp/lerp) | 0 | mini-spec RULED (F24, `docs/tower-mini-spec.md`); all → A8 |
| text (§3) | 14 | 4 (`len contains char_at find`) | rest unsequenced |
| seq (§4) | 25 | 11 (`len contains index_of min max first last push pop insert remove`) | incl. trio + `each`/`map_each`/`filter_map` + `sorted_by` (F0 ruling); `reverse` per naming convention ⚠F26; `sort`/`sort_by`/`sorted`/`sorted_by` → A4 |
| maps (§5) | 8 | 7 (`len keys values get contains_value remove clear`) | `contains_key` not dispatched; `insert` reserved (never ships); 2 operators built |
| flags (§6) | 14 | 0 | unsequenced (frozen LIST ops remain the surface) |
| rand (§7) | 8 + `nonempty` validator | 8 (`float chance pick shuffle shuffled seed` + A5's `int(range)`/`non_empty` + A7's `roll`) | — |
| collections (§8) | 3 heap verbs | 3 (`heap_push heap_pop heap_peek`) + `Weighted[T]`/`weighted` (A7) | v1 = construct-and-roll / push-pop-peek |
| **protocols (§9.6)** | 3 (`display`,`compare`,`iterate/next`) | machinery only (A3: E113/E114/E115, structural defaults; no impl spelling) | not verbs — protocol methods |

**Ruled verb total ≈ 94** (the old "≈93 shipped" mislabeled this — it
was always the *specified* surface; +1 for the F0-added `sorted_by`).
**Built on main: 28 table rows / 24 distinct verb names** (the ✅ rows
above), which together with the conversions `int`/`float`/`string`
(§10), the T1c call forms `call`/`bind`, and the Option constructors
`some`/`none` make up the **29 dispatched call names** in
`is_t1b_stdlib_name` (see the as-built status header). Exact ruled
count still firms as the ⚠-marked signature questions resolve.
