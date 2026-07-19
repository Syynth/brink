# Phase C — the stdlib inventory tables (DRAFT)

Drafted 2026-07-19 from the fully-ruled `docs/stdlib-spec.md` §§1–9
(branch `origin/docs/stdlib-spec-domains-4-7`), the native-surface
charter §§11–13, the effects/T2 spec, the typed-mode spec, issues
#1087/#1097/#1103, and every 2026-07-18 decision-log entry.

**This is a composition draft, not a ruling.** Where a cell could not
be filled from ruled text it is marked `⚠Fn` pointing at the numbered
finding in `phase-c-findings.md`. Blocking findings mean the marked
cells are provisional.

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

## 2. Domain 1b — the numeric tower (§2b) — PARTIAL (mini-spec owed ⏳)

Closed compiler-known tower: **vec2 / vec3 / vec4 / quat** (+ matrices,
sizes ⏳). f32 components (glam-aligned). Global type names like `int`;
verbs live in `std::math`. **No user operator overloading** — the closed
tower is the middle ground. The tower rows below are what is *ruled*;
the rest (value kinds, wire, codecs, marshal legs, NaN/equality
composition, majorness/handedness, save posture) is the **owed
mini-spec** (⚠F24) — these rows are provisional.

| Verb / op | Signature / rule | NS | Pre | Row | Notes |
|---|---|---|---|---|---|
| `dot` | `fn dot(a: vecN, b: vecN): float` | math | ✓* | P | vec2/3/4 |
| `cross` | `fn cross(a: vec3, b: vec3): vec3` | math | ✓* | P | vec3 only |
| `+` `-` | componentwise | (op) | — | P | vecN, quat |
| `*` scalar | `vecN * float`, `float * vecN` scale | (op) | — | P | |
| `mat * vec` | transform | (op) | — | P | matrices ⏳ |
| `quat * quat` | compose | (op) | — | P | |
| `quat * vec` | rotate | (op) | — | P | |
| `min`/`max`/`clamp`/`lerp` | componentwise across the tower (width-1 scalar floor is §1) | math | ✓ | P | "defined once across it" (§2b) |

\* prelude flag for tower verbs inherits the "entire math kit" prelude
ruling; confirm `dot`/`cross` are intended prelude entries with the
mini-spec.

Construction: `vec3 { x: 1.0, y: 2.0, z: 3.0 }` (grammar §K); equality
& display composition ⚠F24.

---

## 3. Domain 2 — text (§3)

Characters = Unicode scalar values (USV); graphemes out of core.
Casing = locale-independent Unicode simple mapping. Prelude:
`len contains char_at`; rest `std::text`. `slice`/`trim`/`split` return
**Views** (§3b — a representation, not a type; row is `P`, view-ness is
unobservable). No protocol deps except structural `eq` for `contains`.

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | Proto |
|---|---|---|---|---|---|---|---|
| `len` | `fn len(s: string): int` | text | ✓ | val | P | — | — |
| `char_at` | `fn char_at(s: string, i: int): string` | text | ✓ | val | `F:oob` | — | — |
| `slice` | `fn slice(s: string, start: int, end: int): string` | text | · | val | `F:oob` ⚠F13 | — (View) | — |
| `contains` | `fn contains(s: string, sub: string): bool` | text | ✓ | val | P | — | eq |
| `starts_with` | `fn starts_with(s: string, prefix: string): bool` | text | · | val | P | — | — |
| `ends_with` | `fn ends_with(s: string, suffix: string): bool` | text | · | val | P | — | — |
| `find` | `fn find(s: string, sub: string): Option[int]` | text | · | val | P | **Option[int]** (martyr #1) | — |
| `replace` | `fn replace(s: string, from: string, to: string): string` | text | · | val | P | — | — |
| `split` | `fn split(s: string, sep: string): [string]` | text | · | val | P | — (Views) | — |
| `join` | `fn join(parts: [string], sep: string): string` | text | · | val | P | — | — |
| `trim` | `fn trim(s: string): string` | text | · | val | P | — (View) | — |
| `repeat` | `fn repeat(s: string, n: int): string` | text | · | val | P ⚠F25 (n<0?) | — | — |
| `upper` | `fn upper(s: string): string` | text | · | val | P | — | — |
| `lower` | `fn lower(s: string): string` | text | · | val | P | — | — |

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

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | Proto |
|---|---|---|---|---|---|---|---|
| `len` | `fn len(a: [T]): int` | seq | ✓ | val | P | — | — |
| `first` | `fn first(a: [T]): Option[T]` | seq | · | val | P | **Option[T]** (empty→none) | — |
| `last` | `fn last(a: [T]): Option[T]` | seq | · | val | P | **Option[T]** | — |
| `index_of` | `fn index_of(a: [T], x: T): Option[int]` | seq | · | val | P | **Option[int]** (martyr #2) | eq ⚠F21 |
| `contains` | `fn contains(a: [T], x: T): bool` | seq | ✓ | val | P | — | eq ⚠F22 |
| `slice` | `fn slice(a: [T], start: int, end: int): [T]` | seq | · | val | `F:oob` ⚠F13 | — (View) | — |
| `concat` | `fn concat(a: [T], b: [T]): [T]` | seq | · | val | P | — | — ⚠F27 |
| `sort` | `fn sort(ref a: [T]): void` | seq | · | lval | `F:float` | — | compare |
| `sort_by` | `fn sort_by(ref a: [T], cmp: fn(T,T): int): void` **⚠F0** | seq | · | lval | `⊕cmp` + inconsistency-fault ⚠F14 | — | — |
| `sorted` | `fn sorted(a: [T]): [T]` | seq | · | val | `F:float` | — | compare |
| `reverse` | `fn reverse(ref a: [T]): void` ⚠F26 | seq | · | lval | P | — | — |
| `reversed` | `fn reversed(a: [T]): [T]` | seq | · | val | P | — | — |
| `min` | `fn min(a: [T]): Option[T]` | seq | · | val | `F:float` | **Option[T]** ⚠F2,F11 | compare |
| `max` | `fn max(a: [T]): Option[T]` | seq | · | val | `F:float` | **Option[T]** ⚠F2,F11 | compare |
| `push` | `fn push(ref a: [T], x: T): void` | seq | ✓ | lval | P | — | — |
| `pop` | `fn pop(ref a: [T]): Option[T]` | seq | · | lval | P | **Option[T]** (empty→none) | — |
| `insert` | `fn insert(ref a: [T], i: int, x: T): void` | seq | · | lval | `F:oob` | — | — |
| `remove` | `fn remove(ref a: [T], i: int): ???` **⚠F5b** | seq | · | lval | `F:oob` | — | — |
| `each` | `fn each(a: [T], f: fn(T): void): void` | seq | · | val | `⊕f` (writes/emits/tags/faults all compose) | — | — |
| `map` | `fn map(a: [T], f: fn(T): U): [U]` | seq | · | val | `⊕f` (reads+faults only; f pure-required) | — | — |
| `filter` | `fn filter(a: [T], pred: fn(T): bool): [T]` | seq | · | val | `⊕pred` (pure-required) | — | — |
| `fold` | `fn fold(a: [T], init: U, f: fn(U,T): U): U` | seq | · | val | `⊕f` (pure-required) | — | — |
| `filter_map` | `fn filter_map(a: [T], f: fn(T): Option[U]): [U]` | seq | · | val | `⊕f` (pure-required) ⚠F9 | callback returns Option | — |
| `map_each` | `fn map_each(a: [T], f: fn(T): U): [U]` | seq | · | val | `⊕f` (full: writes/emits/tags; sequential, never fused) | — | — |

**🔶 `for ref m in maps { m[k] = v }`** — mutating iteration (loop form,
not a verb; index-desugar over RMW; #829 projections stay icebox).

---

## 5. Domain 4 — maps (§5)

Type `[K: V]`, literal `Map { k: v }`. Keys = scalar map-key set + unit
enum variants (non-scalar key = E076 lineage). Prelude: `len` only; rest
`std::map`. `m[k]` read-faults, `m[k] = v` inserts (operators, #856).

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | Proto |
|---|---|---|---|---|---|---|---|
| `len` | `fn len(m: [K: V]): int` | map | ✓ | val | P | — | — |
| `contains_key` | `fn contains_key(m: [K: V], k: K): bool` | map | · | val | P | — | — |
| `contains_value` | `fn contains_value(m: [K: V], v: V): bool` | map | · | val | P (O(n)) | — | eq |
| `get` | `fn get(m: [K: V], k: K): Option[V]` | map | · | val | P | **Option[V]** (martyr #3) | — |
| `keys` | `fn keys(m: [K: V]): [K]` | map | · | val | P (insertion-order snapshot) | — | — |
| `values` | `fn values(m: [K: V]): [V]` | map | · | val | P (insertion-order snapshot) | — | — |
| `remove` | `fn remove(ref m: [K: V], k: K): void` | map | · | lval | P (idempotent-total) | — | — |
| `clear` | `fn clear(ref m: [K: V]): void` | map | · | lval | P | — | — |
| `insert` | **RESERVED — not shipped** (write-index is insertion; verb-form demand = #1103/code-dialect exhibit #1) | map | — | — | — | — | — |
| *(operator)* `m[k]` | read | — | — | val | `F:oob`(missing key) | — | — |
| *(operator)* `m[k] = v` | insert-or-update | — | — | lval | P | — | — |

Pair iteration: **`for k, v in m`** — desugars to key-iteration +
`let v = m[k]` (⚠F10 — exact lowering + mutation-during-iteration owed).
No `entries` verb; `keys`/`values` reify the two projections.

---

## 6. Domain 5 — flags (§6) — the LIST-op audit

Post-rename surface for `flags` (ordered domain of named symbols,
subset-valued variables). `Mood` below stands for any flags type.
Prelude: `contains count`; rest `std::flags`. Two-surface: the frozen
ink LIST ops keep total-empty semantics; the ✚ native verbs return
`Option` on absence.

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

| Verb | Signature | NS | Pre | Recv | Row | Option/refine | Proto |
|---|---|---|---|---|---|---|---|
| `int` | `fn int(r: <inhabited range>): int` | rand | · | — | `W:rng` | **consumes inhabited-range refinement** (total by type) | — |
| `float` | `fn float(): float` → [0,1) | rand | · | — | `W:rng` | — ⚠F4 | — |
| `chance` | `fn chance(p: float): bool` | rand | · | — | `W:rng` **⚠F3** (domain of `p` unruled) | — | — |
| `pick` | `fn pick(it: iterable[T]): Option[T]` | rand | · | lval/val | `W:rng` | **Option[T]** (empty→none) ⚠F18 | iterate |
| `shuffle` | `fn shuffle(ref a: [T]): void` | rand | · | lval | `W:rng` | — | — |
| `shuffled` | `fn shuffled(a: [T]): [T]` | rand | · | val | `W:rng` | — | — |
| `seed` | `fn seed(n: int): void` | rand | · | — | `W:rng` | — | — |
| `roll` | `fn roll(w: Weighted[T]): T` | rand | · | val | `W:rng` | — (total by construction — §8) | — |
| `nonempty` | `fn nonempty(r: range): Option[<inhabited range>]` | rand? ⚠F7 | · | val | P | **produces the refinement** (validator) | — |

**Frozen siblings**: ink `RANDOM(min, max)` / `SEED_RANDOM` are frozen
spellings of the same cell — one RNG, two surfaces, no drift.

---

## 8. Domain 7 — collections+ (§8)

NS `std::collections`. Heap = verbs over ordinary `[T]` (min-heap; zero
new value kinds). `Weighted[T]` is a parameterized builtin.

| Verb / type | Signature | NS | Pre | Recv | Row | Option/refine | Proto |
|---|---|---|---|---|---|---|---|
| `heap_push` | `fn heap_push(ref a: [T], x: T): void` | collections | · | lval | `F:float` (NaN entry-check §4b) | — | compare |
| `heap_pop` | `fn heap_pop(ref a: [T]): Option[T]` | collections | · | lval | P | **Option[T]** (empty→none) | compare |
| `heap_peek` | `fn heap_peek(a: [T]): Option[T]` | collections | · | val | P | **Option[T]** | compare |
| `Weighted[T]` | type; literal `Weighted { weight: value }` | collections | — | — | — (construction refuses empty/zero/neg — evidence-by-construction; NEW diagnostic owed) | evidence refinement ⚠F17 | — |
| `roll` | (in `std::rand`, §7) | rand | · | val | `W:rng` | total by construction | — |

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
| `display` | `fn display(self: T): string` | row ⊆ **pure·silent·total** | structural default for enums/structs; user impl overrides; machine states inherit (#905) | feeds the §1.6 display boundary; ⚠F1 (does `string()` route through it?), ⚠F6 (shadowability) |
| `compare` | `fn compare(a: T, b: T): int` | row ⊆ **pure·silent·total** | none (no structural auto-order; derive-by-fields evidence-gated) | slots into the §4b ordering doctrine; **owed coherence line**: user `compare == 0` need not imply structural `==` ⚠F15 |
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
ordering protocol-gated) is the seat of ⚠F15.

---

## 10. The two-surface table (frozen-ink ↔ native)

The `int()`/`INT()` precedent generalized: one runtime op, two source
surfaces, oracle byte-identical. The frozen surface keeps ink's legacy
semantics untouched; the native surface carries the ruled postures.

| Native (posture) | Frozen ink sibling | Divergence |
|---|---|---|
| `int(x)` / `float(x)` (fault on parse-fail, E078) | `INT()` / `FLOAT()` (silent-0-on-fail) | fault vs silent-zero |
| `string(x)` (total, display form) | (interpolation `{x}`) | same display form ⚠F1 |
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
| tower | `vec3 { x:, y:, z: }` | components ⚠F24 | — |

Struct pattern in `let` reuses match's grammar (§1.3).

---

## 12. Value refinements (§7 doctrine)

CLOSED, compiler-known refinements only — the checker mints the
evidence; **no user-written predicates** (liquid-types = own future
ledger; population today: **two**).

| Refinement | Introduced by | Construction | Consumers | Gradual mode |
|---|---|---|---|---|
| **inhabited range** | `1..=6` literal (const-folded bounds coerce free); `(a..b).nonempty()` for computed bounds | statically-empty literal (`0..0`) = compile error; computed → `Option[<inhabited range>]` | `rand::int` (total by type) | ⚠F8 (does the check exist in gradual?) |
| **Weighted-by-construction** | `Weighted { … }` literal | empty/zero/negative refused: compile error where classifiable, **construction fault** (NEW diagnostic owed) for computed weights | `rand::roll` (total by construction) | construction fault carries to gradual (E078-style split) |

Recorded evolution (not built): a validating `Option`-returning
constructor verb for `Weighted` if dynamic table-building shows demand —
the way `nonempty()` killed the range construction-fault residual.

---

## 13. Verb count

| Domain | Verbs (shipped) | Notes |
|---|---|---|
| math scalar (§1) | 20 | + 2 constants (PI, TAU); `/`,`%` operators frozen |
| numeric tower (§2b) | 2 verbs (`dot`,`cross`) + operators + 4 tower-wide (min/max/clamp/lerp) | PARTIAL — mini-spec owed |
| text (§3) | 14 | |
| seq (§4) | 24 | incl. trio + `each`/`map_each`/`filter_map`; `reverse` per naming convention ⚠F26 |
| maps (§5) | 8 | + `insert` reserved (not shipped); 2 operators |
| flags (§6) | 14 | |
| rand (§7) | 8 + `nonempty` validator | `roll` lives here too |
| collections (§8) | 3 heap verbs | + `Weighted[T]` type |
| **protocols (§9.6)** | 3 (`display`,`compare`,`iterate/next`) | not verbs — protocol methods |

**Shipped verb total ≈ 93** (math 20 + tower 6 + text 14 + seq 24 +
maps 8 + flags 14 + rand 9 heap-adjacent counted once − `roll` overlap
+ heap 3), plus 3 protocol methods, plus 2 constants, plus the reserved
`insert`. Exact count firms once the tower mini-spec and the ⚠-marked
signature questions resolve.
