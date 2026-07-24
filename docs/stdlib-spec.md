# Stdlib spec — DRAFT (checkpoint, 2026-07-19)

Status: **the stdlib sitting in progress** — Phase A postures RULED
(amended 2026-07-18: the Option package, §1.1/§1.4/§1.6); domains
1–3 ruled as marked with the 2026-07-18 absence flips applied;
domains 4–7 + closers DRAFTED as proposals (§§5–9, updated to
conform to the Option ruling, otherwise un-nodded). This
document is the resumption point for any session (cloud included):
read the native-surface charter (§11–§13) first, then this
top-to-bottom; open items are marked ⏳, proposals awaiting the
maintainer's nod are marked 🔶. (Charter numbering gap flagged: no
§12 exists — headers jump §11→§13, and §13.1's "§12.8" reference
dangles; charter fix owed.)

## 1. Phase A — the postures (RULED)

1. **Errors: totality-first + faults; Option NOW; Result
   deferred-with-intent.** Design every verb total where honest;
   turn-terminating faults for true domain errors (the E078
   lineage). **RULED 2026-07-18 (supersedes the martyr strategy):
   `Option[T]` is pulled forward as the third compiler-known
   parameterized builtin (§1.4) — a compiler-owned enum, NO user
   generics unlocked (the `[T]`/`[K: V]` door; evidence + shape on
   #1090). Doctrine: a fault says "your program is wrong"; Option
   says "the world didn't have one." The martyrs are redeemed —
   `find`/`index_of`/map-`get`/`min`/`max` return Option (§§3–5);
   their pressure arriving mid-draft IS the ledger evidence.**
   Result stays deferred (#1097's fault-row bridge is its designed
   arrival).
2. **Fallibility is an effect dimension** (#1097): every verb's spec
   includes its full row — reads/writes/emits/tags/**faults** — the
   keyword-generics vaccine. `@[effects(total)]` assertable. Wake
   conditions trend toward pure·silent·total.
3. **Protocols**: `for x in e` over a CLOSED iterable set = arrays,
   ranges (`0..n`, `0..=n` join the language), flags (active members
   in declared order), maps **by keys**. Strings deliberately don't
   iterate v1 (grapheme question). Indexing: `a[i]` faults OOB;
   `m[k]` read-faults/write-inserts (#856). Construction grammars:
   `Map { k: v }`, `Flags { members }`, arrays `[…]`, structs by
   fields (+defaults). Struct patterns in `let` (match's grammar).
4. **Intrinsic typing doctrine**: parameterized BUILTINS join the
   static type language — `[T]` arrays, `[K: V]` maps, and
   `Option[T]` (RULED 2026-07-18) (syntax mirrors literals;
   statically homogeneous; NOT user generics — #1090 candidate
   (b), promoted). Intrinsics carry checker-known polymorphic
   signatures (closed set); UFCS completion reads them. A bare
   `none` needs a type from context (concrete sites fine; a fresh
   un-annotated `var x = none` errors — the empty-collection
   posture). Docs display notation ⏳ (decide with the inventory).
5. **Namespace**: `std::` tree (`math`, `text`, `seq`, `rand`,
   `collections` — layout finalizes with the inventory); a curated
   ambient PRELUDE (marked per-verb below); `host::` mounts from the
   capability manifest (paths declared per entry); in-tree
   `extern fn` = same species, `story::`-side.
6. **Absence & the display boundary (RULED 2026-07-18 — ships as
   one package with Option).** (a) `x or default` — the coalescing
   spelling, total by construction, the value-position 90% case.
   (b) **Display-boundary forgiveness**: an interpolation whose
   FINAL value is None renders as nothing — absence renders as
   absence, the honest narrative meaning. Everywhere else (guards,
   arithmetic, bindings, arguments — anywhere in an A*
   implementation) `Option[T]` ≠ `T`, strict. Cut by POSITION, not
   dialect: nested compositions are never forgiven
   (`{mood.first() + 1}` is a type error; only the boundary
   shrugs). Riders: the forgiveness is TRACEABLE (transcript/debug
   records None-renders; an always-None-interpolation lint ⏳);
   choice-text and tag surfaces are named edges (accidental empty
   choice text vs the deliberate `* []`) ⏳. **F27 RULED
   (2026-07-19): Option has NO truthiness** — a condition-position
   `Option[T]` is a compile error under strict and a runtime fault
   under gradual; the author writes `== none`, `== some(x)`, or
   (post-B1) the `as`-binding. Truthiness is a quiet coercion of
   exactly the kind `Option[T] ≠ T` exists to ban. (Supersedes
   A1's shipped falsy-none — implementation fix owed.) **F28 RULED
   (2026-07-19)**: `none`/`some(…)` render totally in display
   until B4's boundary-forgiveness arrives; `string()`'s ruled
   totality is preserved.

## 2. Domain 1 — math (RULED)

Scalar kit: `abs sign min max clamp lerp inverse_lerp smoothstep
floor ceil round trunc sqrt pow sin cos tan atan2`, constants `PI
TAU`. All pure·silent·total. NaN-totality (IEEE; `sqrt(-1.0)` =
NaN, never a fault). Int/float via checker-known intrinsic
overloads (the doctrine's first exercise). Heritage `/` `%` stay
truncating (ink-frozen); **`div_floor` + `mod_floor`** are the
blessed grid verbs. **Prelude: the entire kit, trig included**
(generous ruling).

### 2b. The numeric tower (RULED — resolves #827)
Closed, compiler-known native tower: **vec2 / vec3 / vec4 / quat**
(+ matrices, sizes ⏳ mini-spec). GLSL conventions: `+`/`-`/`*`
componentwise, scalar scale, `dot`/`cross` as verbs, `mat * vec`
transforms, `quat * quat` composes, `quat * vec` rotates. **f32
components** (glam-aligned; the bevy boundary marshals
structurally). No user operator overloading — the closed tower IS
the middle ground. Scalar kit = the tower's width-1 floor
(lerp/clamp/min/max defined once across it — Mojo's lesson without
its machinery). Mini-spec RULED 2026-07-19 — see
docs/tower-mini-spec.md (glam-backed value kinds, all matrix
sizes, lane-serialized wire, componentwise-IEEE equality,
not-orderable).

## 3. Domain 2 — text (RULED)

`len char_at slice contains starts_with ends_with find replace
split join trim repeat upper lower`. **Characters = Unicode scalar
values** (indexing verbs count/index by USV; graphemes are
explicitly a RENDERING concern — out of core, ledger-worthy if
authored code ever needs them). Casing = locale-independent Unicode
simple mapping (Turkish-i named as out of scope; locale casing =
intl pipeline). `char_at`/`slice` OOB fault (one indexing contract
with arrays — OOB indexing is a bug, not absence). `find` →
`Option[int]` (martyr #1, redeemed by the 2026-07-18 ruling; the
-1 sentinel dies unshipped). Prelude: `len contains char_at`;
rest `std::text`.

### 3b. Views (RULED)
**Views are a representation, not a type**: string/array
`slice`/`trim`/`split` return `View { base: Arc, range }` internally
— legal because sharing is unobservable (law_cow_sharing); every
observation is repr-transparent. Spec'd as a PERFORMANCE CONTRACT
("O(1), non-allocating"), regression-guarded via bench-counters
(arc_clones/cow_copies). The Java-substring leak is named: wire/
saves always materialize; creation applies a view≪base size
heuristic (ratio ⏳). Distinct from #829's REF projections (the
mutating write-through cousin, still icebox, reserved wire slot) —
cross-referenced so views ≠ projections.

## 4. Domain 3 — sequences & iteration (one design)

- One closed iterable set serves `for` and the verbs alike.
- **Fn-value trio `map filter fold` — PURE-REQUIRED (RULED
  2026-07-18, superseding the earlier eager-vs-lazy framing).**
  Callbacks must be **pure·silent** (reads legal — filtering on
  story state is the bread-and-butter case; totality NOT required
  — a faulting callback is legitimate). Consequence: stage
  interleaving is unobservable BY CONSTRUCTION, so the spec says
  "one logical pass, order unobservable; the implementation may
  fuse freely" — **the eager/lazy question is dissolved, not
  deferred**; laziness is an implementation detail forever. The
  trio is algebra, not control flow; the #672-B property laws
  hold unconditionally. The only fusion-visible artifact: WHICH
  element's fault fires first when several would — unspecified.
  Rows still compose for what remains (the #872 machinery):
  `a.map(f)` faults iff `f` can, reads what `f` reads. This is
  the established position-demands-row pattern (wake conditions,
  display/compare impls). `filter_map(f)` where
  `f: fn(T): Option[U]` is the Option-mapper (drops nones) — the
  natural companion under the Option ruling.
- **Effectful iteration is a different concept and gets different
  spellings**: `each` (do something per element, no result), and
  **`map_each`** — the effectful transform (produces the array,
  callback may write/emit; sequential in iteration order, defined
  element-by-element, never fused). **Naming principle (RULED
  with it): the weird thing gets the ugly method** — convenience
  is spent on the pure spelling (`map`), friction on the
  effectful one (`map_each`); the name is the speed bump. The
  trio's rejection error names both exits: "make it pure, or say
  map_each." Further `_each` variants only on evidence.
- 🔶 **Mutating iteration — `for ref m in maps { m[k] = v }`**
  (proposed; arose from the §5 array-of-maps case): a ref-binding
  in `for` over arrays, desugaring to index-based access so
  writes ride the existing RMW machinery — no element-projection
  machinery (#829 stays icebox), lvalue rules apply. The
  loop-shaped answer to "mutate each element"; a fn-value
  mutating-each would need projections and is NOT proposed.
- **Push/pull without laziness (RULED)**: (1) internal iteration —
  `each`/`for_each` — free today; (2) fused verbs (`filter_map`)
  for hot 2-stage chains; (3) **row-gated fusion**: the compiler
  may fuse intrinsic chains when callback rows prove PURITY (fusion
  only changes effect interleaving ⇒ purity makes it invisible) —
  the effect system licenses deforestation. True pull-iterators =
  future protocol; flows already serve coroutine-shaped needs.
- **Mutation posture (RULED 2026-07-18)**: mutating verbs take
  `ref` first params; **UFCS auto-refs an lvalue receiver**
  (`inventory.push(sword)`; field paths write through RMW). An
  **rvalue receiver is a compile error** (`[1,2].push(3)`,
  `a.sorted().push(x)` — mutating a temporary loses the
  mutation). The free-call form stays explicit
  (`push(ref inventory, sword)`) — the sugar is earned in method
  position, the spelled form teaches what it means. Safe
  sigil-free because values are COW (no aliasing/escape to warn
  about) and the mutation lives in the effect row regardless.
  **Naming (RULED with it)**: imperative = in-place (`sort push
  insert remove reverse`), past-participle = functional (`sorted
  reversed`) — the verb carries the mutation signal; the
  confusion lattice closes from both sides (`let b = a.sort()` =
  unit type error; `a.sorted().push(x)` = rvalue error).
- Verbs: `len first last index_of contains slice(view) concat
  sort sort_by sorted sorted_by reversed min max push pop insert remove
  each map filter fold filter_map`. **Absence returns (RULED
  2026-07-18, flipping the earlier empty⇒fault posture — one
  doctrine, no day-one exceptions)**: `first last min max pop` →
  `Option` on empty; `index_of` → `Option[int]` (martyr #2,
  redeemed). OOB *indexing* (`a[i]`, `insert`, remove-by-index)
  stays a fault — an index you computed wrong is a bug; an empty
  extremum is absence. Prelude: `len contains push`; rest
  `std::seq`.
- **The `list` reclaim dissolves**: type is `[T]`, literal `[…]`,
  vocabulary is "array"; the word "list" RETIRES entirely.

### 4b. The ordering doctrine (RULED 2026-07-18)

Serves `sort`/`sort_by`/`min`/`max`/heap (§8) and `compare` (§9.6).

- **NaN flows through arithmetic (§2's NaN-totality untouched);
  ordering contexts are where it stops.** DEV mode: a NaN operand
  in `sort`/`sort_by`/`min`/`max`/`heap_push` is a
  turn-terminating fault — the upstream bug surfaces at its first
  ordering consumption. PROD mode: the pinned total order applies
  — ordinary IEEE order with `-0 == +0` as a tie, NaN greater
  than everything, NaN-vs-NaN ties. On NaN-free data the modes
  agree exactly and cohere with `<`/`==` — zero
  compare-vs-equality divergence on clean data (deliberately NOT
  IEEE totalOrder, which would split `-0`/`+0` from `==`).
  *(Amended 2026-07-19, NS-A4 as-built per F14: `sort_by` does NOT
  belong in the dev NaN-fault list above — the comparator owns the
  element semantics, NaN never reaches the ordering machinery as a
  comparison result; its faults are `⊕cmp` ∪ dispatch ∪ detected
  inconsistency. The list reads `sort`/`sorted`/`min`/`max`/
  `heap_push`.)*
- **The dev/prod split is FENCED**: available only where the prod
  behavior is defined, total, and **fabricates no data** —
  placement qualifies (every element preserved, order
  deterministic, saves/replay-safe); fabrication never does
  (`int("potato")`, OOB indexing stay always-fault). Checked int
  overflow is a noted sibling candidate for the same knob — not
  ruled. Knob home RULED 2026-07-19: project config
  (brink.toml profile) with a host-API override; tooling
  implements when A4 needs it.
- **Rows are mode-independent**: ordering verbs over `[float]`
  carry `faults` unconditionally (the conservative union — prod
  never fires it; the checker doesn't know modes exist).
  `[int]`/`[string]`/`[bool]` orderings are total. Totality-gated
  positions (wake conditions) therefore flag float orderings in
  both modes — correct: a NaN-able wake condition is a landmine
  regardless of build profile.
- **What orders**: int · float (above) · bool (`false < true`) ·
  strings lexicographic by USV (§3's unit; locale collation = the
  intl pipeline's business, like casing) · arrays lexicographic
  element-wise (elements recursively orderable; same NaN rule
  inside). Structs/enums: ONLY via an explicit registry `compare`
  impl (§9.6) — no structural auto-order (field declaration order
  must not silently define semantics; derive-by-fields is
  evidence-gated future). Not orderable: maps, flags subsets
  (partial order), divert targets.
- **Comparison operators stay frozen IEEE** (`NaN < x` false,
  `NaN == NaN` false — ink-inherited, oracle-guarded, total).
  Only the stdlib verbs carry the doctrine — the two-surface
  pattern's third application.
- **F0 RULED (2026-07-19)**: `sort_by` is IN-PLACE
  (`ref a`, imperative per the naming law); `sorted_by` is its
  functional past-participle twin; the §9.4 display exemplar is
  reselected to `map` so the notation banner stops teaching a
  convention violation.
- **`sort_by`**: the comparator falls under the trio's
  pure·silent rule plus the consistent-total-order LAW; the
  implementation may fault on detected inconsistency; the
  guarantee floor is "some permutation of the input, never
  worse." `heap_push` checks at entry — the invariant then holds
  over clean data.

  **RULED by delegation 2026-07-19 (not fully reviewed — see the decision-log's "Delegated batch ruling" entry):** **F29(a)** — the symmetric
  carve-out: a protocol `display`/`compare` impl whose inferred row is
  provably total does NOT inherit the conservative faults bit; the
  conservative union applies only when the impl's own row is opaque or
  fault-bearing. (The post-A3 composition audit's C1/C2 finding; gates A4.)

## 5. Domain 4 — maps (RULED 2026-07-18)

- Type `[K: V]` (doctrine §1.4); literal `Map { k: v }` (Phase A
  §1.3). Statically homogeneous both legs. **Key domain = the
  scalar map-key set + unit enum variants** (charter §13.1); a
  non-scalar key is the E076 lineage at compile time where
  classifiable.
- Indexing contract stands as ruled (#856): `m[k]` read faults on
  a missing key; `m[k] = v` inserts. No `insert` verb ships —
  write-index IS insertion (one spelling per concept) — but
  **`insert` is RESERVED** as the designated verb-form name:
  syntax isn't passable, so value-position demand arrives with
  lambdas/pipelines; it ships (or dissolves into something
  better) with the code-dialect sitting's syntax-in-value-position
  decision, where it is exhibit #1 (§10).
- **The non-faulting read (updated per the 2026-07-18 Option
  ruling)**: `get(m, k)` → `Option[V]` — martyr #3, redeemed
  before it was ever martyred. `m.get(k) or default` covers the
  with-default idiom, so no `get_or` verb ships (the `or`
  spelling subsumes it — one spelling per concept); `contains_key`
  stays for the pure membership test. The faulting `m[k]` remains
  the "I expect it there" read (#856 unchanged).
- Verbs: `len contains_key contains_value get keys values remove
  clear`. `contains_value` — equality scan over values (content
  equality per the ruled semantics), total, O(n) and honest about
  it; the `contains_key`/`contains_value` pair kills the
  ambiguity bare `contains` would carry on maps.
  `remove(ref m, k)` imperative/in-place per the mutation posture,
  total (removing an absent key is a no-op — deletion is
  idempotent; the faulting read covers "I expected it there").
  `clear(ref m)` in-place, total.
  `keys`/`values` → eager array snapshots in insertion order
  (iteration order is already insertion order; equality alone
  ignores it, per the 2026-07-18 ruling).
- **No `entries` verb** (updated by the §9.1 closer ruling):
  **`for k, v in m`** two-binding iteration is the pair story —
  desugars to key-iteration + `let v = m[k]`, total by
  construction, no pair shape ever materializes. F10 RULED
  (2026-07-19): the key set is SNAPSHOTTED eagerly at loop entry —
  maps' `for` is a deliberate exception to live pull iteration;
  structural modification mid-loop sees the snapshot's keys, and a
  snapshotted key since removed hits the faulting read honestly. A reified
  `entries()` array is evidence-gated.
- `for k in m` iterates keys (ruled); `m.keys()` is the same set
  reified. Prelude: `len` only (already ambient); the rest
  `std::map`.

## 6. Domain 5 — flags (RULED 2026-07-18): the LIST-op audit

Post-rename surface for `flags` (charter §13.2: ordered domain of
named symbols, subset-valued variables). The audit disposes every
inherited LIST operation. **The respelling-vs-new-fault
controversy this table originally carried is DISSOLVED by the
2026-07-18 Option ruling**: absence-shaped results return
`Option`, which is neither ink's silent-empty nor a fault — the
rows marked ✚ are new native verbs whose absence returns are
typed; the frozen ink ops keep their total-empty semantics
untouched (two surfaces, one runtime — the `int()`/`INT()`
precedent, oracle byte-identical):

| ink inheritance | native | disposition |
|---|---|---|
| `LIST_COUNT` | `count` | keep (verb) |
| `LIST_ALL` | `all(Mood)` | keep — full-domain subset |
| `()` empty | `none(Mood)` | new spelling; empty-subset literal |
| `?` membership | `contains` | keep (verb; operator form ⏳ code-dialect sitting) |
| `+=` / `-=` | `add` / `remove` | keep both operator and verb; `ref` first param, in-place |
| `^` intersection | `intersect` | verb; operator form ⏳ |
| `LIST_MIN`/`MAX` | `first` / `last` | ✚ proposed rename — domain-order vocabulary, not numeric; empty subset → `none` via `Option` (matches the flipped seq `min`/`max`; ink's ops stay frozen-total) |
| `LIST_VALUE` | `index_of` | `index_of(flag)` → int — the honest ordinal query: total on a single flag (every flag has a position); multi/empty input faults (the `next`/`prev` shape). The subsets↔ints *conversion* is frozen (below) |
| `LIST_RANGE` | `range` | keep, `range(Mood, a, b)` inclusive by domain order |
| `LIST_INVERT` | `invert` | keep — complement within the domain |
| `LIST_RANDOM` | moves to domain 6 | ✚ `rand::pick` accepts a flags subset (closed iterable set member); empty → `Option` none (dynamic-content absence — §7); frozen ink op untouched |

- `next`/`prev` (✚) step a **single-flag subset** by domain order,
  returning `Option` — off-the-edge is expected absence (`none`,
  the typed version of ink's honest empty). Stepping a multi-flag
  or empty subset **faults** — a malformed question is a bug, not
  absence (the doctrine's two halves in one verb). Ink's own
  `+1`/`-1` stepping stays frozen.
- **The numeric coupling — FROZEN (RULED 2026-07-18)**: ink's
  explicit numeric values on flags and the subsets↔ints
  conversions stay on the ink-frozen surface, never respelled.
  Native flags are pure ordered symbols; ordinal queries go
  through `index_of` (the author writes it and owns the int);
  "symbol with data" is enums-with-payloads' job. Migrating
  stories that do LIST arithmetic keep that code on the frozen
  surface or rewrite against `index_of` — that idiom is exactly
  the LIST abuse machines and enums now serve properly.
- Prelude: `contains count`; rest `std::flags`.

## 7. Domain 6 — random (RULED 2026-07-18)

- **The effect answer — no new row dimension.** RNG state is a
  named runtime state cell (`std::rand` owns it); every draw is an
  ordinary **write** to that cell in the def's row. Consequences,
  all free: wake conditions (pure-gated) statically exclude rng —
  a re-evaluated draw would be re-roll-unstable, so this is the
  correct exclusion, enforced by existing machinery; row-gated
  fusion (§4) correctly refuses to fuse draw-bearing callbacks;
  `@[effects(pure)]` already asserts rng-freedom. No sibling
  dimension to #1087/#1097 needed — rng is state, not a new kind
  of observation.
- **Determinism posture**: algorithm pinned (implementation
  chooses; stability is the contract), draws are a pure function
  of RNG-state → (value, state'). RNG state saves/loads with the
  story (it already lives in story state — same-semantics). Seeded
  replay = identical transcript, cross-platform. `seed(n)` writes
  the cell; unseeded stories seed from the host at story start
  (host concern, manifest-visible).
- Verbs (`std::rand`, **no prelude entries** — draws are
  deliberate acts, namespaced): `int` · `float()` → [0,1) ·
  `chance(p)` → bool (F3 RULED 2026-07-19: p clamped to [0,1] —
  total; NaN → false; interpretation, not fabrication) · `pick` · `shuffle(ref a)` in-place +
  `shuffled(a)` functional (the ruled §4 naming convention
  exercised again) · `seed(n)`.
- **`rand::int` is total BY TYPE — the invalid case is
  unrepresentable (RULED, reshaped in-sitting).** Its parameter
  is the language's **first value refinement: the inhabited
  range**. A range literal with checker-provable bounds
  (`1..=6`, `5..=5`, CONST refs fold) coerces in free — dice
  cost nothing; a statically-empty literal (`0..0`) is a
  **compile error**; computed bounds must arrive through the
  validator **`(a..b).nonempty()` → `Option[<inhabited range>]`**
  — parse-don't-validate: the Option tax sits once, at the
  boundary where dynamic data enters, then N draws cost nothing
  (amortized; contrast per-draw `pick` coalescing). Plain
  `range` stays possibly-empty everywhere else — `for i in 0..n`
  with n = 0 runs zero times, `pick(0..n)` returns `none` —
  emptiness is load-bearing for iteration and untouched.
- **`pick(iterable)`** → `Option` (any closed-set iterable incl.
  ranges and flags subsets; empty → `none` — dynamic-content
  absence). UFCS makes `enemies.pick()` the direct-collection
  spelling — the case that used to masquerade as
  `rand::int(0..enemies.len())`, now spelled as what it means.
- **Refinement doctrine (recorded with the ruling)**: effect rows
  are refinements on functions; this is the same species applied
  to a value. CLOSED, compiler-known refinements only — the
  checker mints the evidence; no user-written predicates
  (liquid-types territory = its own evidence ledger if demand
  materializes; population today: one). Exact type/validator
  spelling ⏳ code-dialect sitting. **F7 RULED (2026-07-19):
  ranges are a REAL Value kind** — wire form, content equality,
  display, save posture — because FlowFrame spills for-loop
  iterators across `await` and a range iterator needs a durable
  wire form (A5 specifies; the refinement is a refined view over
  this value kind). **F8 RULED (2026-07-19): refinements in
  gradual mode are inert with a runtime-fault residual** —
  `rand::int` faults on an empty range under gradual, the
  compile-time evidence machinery is strict-mode's (the
  `int()`/E078 precedent); recorded as the GENERAL rule for all
  future refinements.
- Heritage: ink's `RANDOM(min, max)` / `SEED_RANDOM` stay
  ink-frozen spellings of the same cell — one RNG, two surfaces,
  no drift.

  **RULED by delegation 2026-07-19 (not fully reviewed — see the decision-log's "Delegated batch ruling" entry):** **F30(a)** — content equality means the
  **denoted integer sequence**: `1..=6 == 1..7` is true and all empty
  ranges are mutually equal, while display and wire preserve the written
  form (the #909 content-over-form precedent). Shipped in A5 (#1136),
  now ratified.

## 8. Domain 7 — collections+ (RULED 2026-07-18)

- **Weighted tables — the dossier's evidenced structure.** A
  parameterized builtin `Weighted[T]`; construction reuses the map
  literal shape with weights as keys: `Weighted { 3: sword,
  1: shield }` (grammar `weight: value` as chartered; weights =
  positive ints v1). One draw verb: `rand::roll(w)` → T — lives in
  domain 6's namespace because its row writes the rng cell.
  **Weighted is evidence-by-construction** (the §7 refinement
  shape arrived at independently): construction refuses
  empty/zero/negative-weight tables — compile error where
  statically classifiable, construction fault for computed
  weights (the E078-style split; the compile diagnostic is a NEW
  code owed by Phase C) — so `roll` over any table that EXISTS is
  total. Designated evolution, recorded not built: if dynamic
  table-building shows dossier demand, a validating constructor
  verb returning `Option` kills the construction-fault residual
  the way `nonempty()` did for ranges. `len`, iteration, and
  mutation ⏳ — v1 is construct-and-roll.
- **Heap/priority queue — the humble form first.** Proposal: verbs
  over arrays, not a new type — `heap_push(ref a, x)`,
  `heap_pop(ref a)` → `Option`, `heap_peek(a)` → `Option` (empty
  is absence, per the 2026-07-18 doctrine — and `while
  heap_pop(ref open) as node { … }` is the natural drain loop),
  maintaining the invariant over an ordinary `[T]`
  (`std::collections`). Rationale: zero new value kinds, zero wire
  work, the Lua posture; min-heap. Ordering per the ruled
  doctrine (§4b): `heap_push` NaN-checks at entry (dev fault /
  prod pinned order). If the
  ledger later shows shape-confusion incidents (heap-array indexed
  as if sorted), a sealed `Heap[T]` builtin is the designed
  upgrade path — recorded, not built.
- Anything further (deque, set-as-type) is **evidence-gated** —
  `std::collections` is the landing zone, the dossier is the gate.

  *(As-built, 2026-07-19, NS-A7 #1113: the owed NEW diagnostic is
  **E120** — fired at the lowering for statically-classifiable refusals
  (empty tables, dangling weights, literal non-positive/non-int weights,
  negated literals included), in BOTH type regimes; computed weights
  carry the `WeightedBadWeight` construction fault. Until B5's
  construction grammar, the brink-dialect spelling is the ambient
  intrinsic `weighted(w1, v1, w2, v2, …)` — the slice-1 pattern, exactly
  as `vec2(…)` stands in for the tower literals. Equality shipped as
  **multiset content** — order-insensitive, multiplicity-sensitive (the
  F17 multiset read through the #909 content-over-form lens); display
  and the roll walk keep construction order. Wire: one `Collect` opcode
  (0xFA + kind byte, the Tower economy) and value tag 0x19.)*

## 9. Closers (RULED 2026-07-18, except as marked)

1. **Anonymous records — RETIRED from the native surface.**
   Homogeneous bags are maps; typed shapes are declared structs
   (cheap — a two-line struct above a `fn` is honest
   documentation); **multi-return = declare the struct** (no
   tuples, no third structural-record concept; records-as-maps
   was rejected on typing — heterogeneous fields die in `[K: V]`
   homogeneity). If declaration ceremony ever genuinely bites,
   lightweight inline record types are a code-dialect-sitting
   question, ledger-gated. The `entries()` question DISSOLVES:
   **`for k, v in m`** two-binding iteration desugars to
   key-iteration + `let v = m[k]` (total — the key just came from
   the map); no pair shape materializes; a reified `entries()`
   array waits for evidence. **Construction syntax**: one
   initializer grammar, `TypeName { … }`, per-type meaning —
   already conformed to by every construction form in this spec.
   Whether that's a per-type *protocol* (C#-lineage, the
   maintainer's recalled earlier-thread direction) or grammar
   dispatch is **#1103** — filed as a come-back-to for the
   code-dialect sitting; this sitting commits only to the
   grammar shape.
2. **Assertion spellings — final form** `@[effects(…)]` with args
   from `{pure, silent, total}`, any subset, comma-joined;
   exceedance-only errors (asserting less than reality is legal).
   `@[effects(pure)]` ⊃ rng-freedom (§7). Clause grammar
   AMENDED 2026-07-19 (the Rust meta-item shape): clause
   arguments are PARENTHESIZED — `@[effects(reads(gold, hp),
   writes(mood), silent)]`; bare idents at top level are always
   flags, so a flag can never be swallowed into an open clause.
   The deprecated `#@effects` alias keeps its legacy colon
   grammar FROZEN (E110-warned surface does not evolve). Doc-sync owed: the
   effects spec and #1087 still show the older `#@effects(…)`
   spelling — supersession note there when this lands. Holes'
   release policy — **PARKED past this sitting by the maintainer**
   (2026-07-18); it survives the closers as the one deliberately
   open authorial-workflow judgment.
3. **Prelude — final list assembled from the per-domain marks:**
   entire math kit incl. trig (§2's generous ruling) · `len
   contains char_at` (text) · `len contains push` (seq) · `len`
   (maps) · `contains count` (flags) · nothing from rand/
   collections. Name-collision policy: prelude names are
   **shadowable with the E035-lineage warning** (stdlib-slice-1
   posture carries over).
4. **Docs display notation for intrinsic signatures**: the
   pseudo-generic letter form — `fn map(a: [T], f:
   fn(T): U): [U]` — with a standing banner: *display
   notation; `T` is not writable in source* (#1090 guards the
   door). Chosen over concrete-example notation because UFCS
   completion already shows this shape; docs and IDE should agree.
5. **`std::` tree — final layout**: `std::math` (tower types are
   global type names like `int`; their verbs live in the kit) ·
   `std::text` · `std::seq` · `std::map` · `std::flags` ·
   `std::rand` · `std::collections` (heap verbs, `Weighted`).
   `host::` mounts per the manifest (charter §13.2); `story::` is
   authored land. No `std::prelude` module — the prelude is a
   compiler-curated name set, not an importable place (imports are
   naming-only; the prelude is pre-granted naming).
6. **The protocol registry (RULED 2026-07-18 — nodded in the same
   conversation; full reasoning on the PR #1100 thread).** A
   CLOSED set of compiler-declared protocols that user types may
   *implement* but never *declare* — no bounds, no user generics;
   the two-tier discipline holds: closed overload families (math
   kit, tower, len/contains) stay mechanism-free intrinsics, and
   registry entries exist only where user types participate in a
   compiler behavior, promotion evidence-gated via #1090. V1
   entries, each with an **effect contract**:
   - `display` — `fn(T): string`, row ⊆ pure·silent·total; feeds
     the §1.6 boundary; enums/structs get structural defaults,
     user impls override. Machine states inherit it (#905).
     **F1 RULED (2026-07-19)**: BOTH interpolation and the
     `string()` conversion intrinsic dispatch through `display` —
     one display path, honoring the 2026-07-13 "same as
     interpolation" guarantee; `string()`'s totality survives
     because the contract forbids faulting impls. **F6 RULED
     (2026-07-19)**: the registry method names (`display`,
     `compare`, `next`) are RESERVED — author shadowing is a hard
     compile error, not an E035 warning; a shadowed `display`
     would make interpolation untrustworthy.
   - `compare` — `fn(T, T): int`, row ⊆ pure·silent·total; user
     impls slot into the RULED ordering doctrine (§4b). Coherence RULED
     2026-07-19: `compare` is ORDERING ONLY — equality stays
     structural always; `compare == 0` need not imply `==`,
     divergence is legal and documented (sort never implies dedup
     semantics); enforceable by construction.
   - `iterate` — **pull-shaped**: `next(ref Self): Option[T]`,
     row ⊆ writes-receiver·silent·total, laws attached ("every
     element once; `none` terminal and sticky" — property-harness
     enforced; machine-form impls make them structural). Chosen
     over push/`each` because a push-desugared `for` body is an
     fn-value callback and **functions never await** — push would
     ban `await` inside `for` bodies in flows; pull desugars
     inline and iterators park across suspensions for free. `for`
     is the only v1 consumer (concrete-site resolution under
     mono-HM, zero generics); user iterables joining
     `map`/`filter`/`fold` stays #1090-gated. `each`/`for_each`
     remain ordinary derived verbs for pure-callback cases.
   - `construct` — the **4th entry (RULED 2026-07-23, #1103)**:
     `TypeName { … }` construction is protocol dispatch (the C#
     `Add`-method lineage), **not** closed compiler grammar over a
     fixed set — so a future collection (`Heap[T]`, host types)
     joins the literal grammar with no grammar change, and it's
     symmetric with `display`/`compare` being protocols rather than
     the lone grammar exception. The brace *tokens* (element / pair
     / field forms) stay fixed surface grammar the parser produces;
     the protocol governs dispatch/meaning only. **This round only
     std types register**; user-type opt-in rides the deferred impl
     spelling. Two members: the **total** literal (`Weighted { … }`
     faults on an invalid table — the 90% value-position case,
     ships now) and a **validating** variant (`construct → Option`,
     for data-driven/runtime tables) — the principled home for
     evidence-by-construction (Weighted's §7 refinement); the
     validating member is **ratified but its user-facing spelling is
     deferred** with the impl spelling. Duplicate keys in a map
     literal (`Map { k:1, k:2 }`) are a **compile error** (new
     E076-lineage code), consistent with struct dup-field. A spread
     / from-existing form (`Map { ..other, k:v }`) is **deferred** —
     no demonstrated demand, extensible later at zero grammar cost.
   Implementation spelling (attribute vs impl-block) ⏳ —
   code-dialect sitting.

## 10. Remaining docket ⏳

- **RULED 2026-07-18 (in-conversation, this session)**: the
  Option package — `Option[T]` as builtin #3, the fault=bug /
  Option=absence doctrine, `or`, display-boundary forgiveness
  (§1.1/§1.4/§1.6) — and the seq/text/map flips (§§3–5). Recorded
  in the decision log.
- **Also RULED 2026-07-18**: the protocol registry (§9.6) —
  closed set, v1 = `display`/`compare`/`iterate` (pull-shaped),
  per-protocol effect contracts, two-tier discipline. Its
  interior ⏳s (impl spelling, ordering doctrine content, the
  compare/equality coherence line) remain open.
- **Also RULED 2026-07-18**: the mutation posture — UFCS auto-ref
  on lvalue receivers, rvalue-receiver compile error, explicit
  free-call form — and the imperative/past-participle naming
  convention (§4).
- **Also RULED 2026-07-18**: the trio is pure·silent-required —
  eager/lazy dissolved by construction; `each`/`map_each` are the
  effectful spellings; "the weird thing gets the ugly method"
  recorded as a naming principle (§4). **Sitting-3's 🔶s are now
  all closed.**
- Still awaiting the nod: §§5–9's remaining proposal content
  (updated to conform to the Option ruling but not themselves
  ruled).
- **Also RULED 2026-07-18**: the ordering doctrine (§4b) — NaN
  flows through arithmetic, faults at ordering contexts in DEV,
  pinned non-fabricating total order in PROD; the dev/prod split
  fenced to placement-never-fabrication; rows mode-independent;
  the orderable-types roster; frozen IEEE operators.
- **Also RULED 2026-07-18**: maps (§5 — `contains_value` added,
  `insert` reserved) · flags (§6 — renames, `first`/`last`,
  `next`/`prev`, `index_of`, numeric coupling FROZEN) · random
  (§7 — rng-as-cell, `rand::int` total by type via the inhabited
  range: the first value refinement, closed-refinement doctrine
  recorded; `pick` Option; determinism posture) · collections+
  (§8 — `Weighted[T]` evidence-by-construction with the
  Option-constructor evolution recorded; humble heap over arrays
  with the sealed-`Heap[T]` upgrade path; further additions
  evidence-gated) · the closers (§9 — anonymous records RETIRED,
  `for k, v in m` replaces `entries`, multi-return = declared
  struct, `@[effects]` final form, prelude final list, display
  notation, `std::` tree; initializer protocol-vs-grammar filed
  as #1103; holes' release policy parked by the maintainer).
- **§§1–9 are now fully ruled.** The sitting's remaining work is
  Phase C.
- In-section ⏳s: tower mini-spec (§2b) · view-materialization
  ratio (§3b) · weighted-table mutation surface (§8) · holes'
  release policy (§9.2, maintainer-parked) · protocol
  implementation spelling + compare/equality coherence line
  (§9.6) ·
  inhabited-range type/validator spelling (§7, code-dialect
  sitting) · initializer protocol-vs-grammar (#1103,
  code-dialect sitting).
- Maintainer-attention note: `remove` now names three verbs with
  divergent postures — seq remove-by-index (OOB ⇒ fault, the
  indexing contract), map remove-by-key (idempotent-total), flags
  subtract (idempotent-total). Legal under intrinsic overloading;
  flagged so the divergence is chosen, not accidental.
- **Lambdas RULED early (2026-07-19, airport sitting — the
  code-dialect opening item closed pre-sitting)**: **Rust pipes
  with colon returns** — `|g| g.awake`, `|g: Guest|: bool { … }`,
  `||` zero-arg — under the RustScript north star (charter §7
  amended). Riders: single-expression or braced-block bodies,
  `return` leaves the lambda, last expression is the value;
  capture BY-VALUE always (= Rust `move` as the only mode, no
  keyword; ratifying the pre-registered bet), no ref captures v1,
  **assignment to a captured binding is a compile error** (a
  snapshot write is always a lost write); lambdas are fn-colored
  always (no `await` inside, per the axiom); params optionally
  annotated, mono-HM infers at concrete sites, bare lambda
  without context = the E107 posture; a named fn passed by bare
  name is the same species of value. Rows compose per #872,
  unchanged.
- **Handed to the code-dialect sitting (recorded 2026-07-18)**:
  (1) ~~Lambda/fn-value literal design~~ RULED above —
  the confession: the entire fn-value verb layer (trio, each,
  map_each, sort_by, iterate laws, #872 composition) presumes
  literals that have never been designed; Phase C must list it
  as an implementation prerequisite. Pre-registered bet the
  verbs leaned on: **capture is by-value** (COW makes it cheap;
  no ref captures v1, so closures can't smuggle mutable aliases
  past the auto-ref rules); rows compose through captures per
  #872. Reopening capture reopens knowingly. (2)
  **Syntax-in-value-position** — one coherent mechanism (operator
  sections vs `(+)`-style operator-values vs named verb twins vs
  nothing): exhibit #1 the reserved `insert` (§5), exhibit #2
  fold-over-`+`.
- Then: **Phase C** — the full inventory tables (every verb ×
  signature × row × prelude flag) appended here, and
  implementation sequencing (the numeric tower, effect-row
  extensions #1087/#1097, and the Option/registry substrate are
  compiler/runtime work that can pump BEFORE the parser exists;
  the surface-syntax-dependent parts wait on the prototype
  parser).

## 11. Session-resumption notes

Ruled context lives in: docs/native-surface-charter.md (§1–§13),
this doc, issues #1087/#1090/#1093/#1097/#905 (FSM sidebar), the
PR #1100 conversation thread, and the decision log. §§1–4 are
ruled as marked (Option package + absence flips ruled
2026-07-18 in-conversation); **§§5–9 are proposals updated to
conform, awaiting the nod** — the maintainer reacts top-to-bottom,
then Phase C convenes. The native-surface prototype parser is the
season's next artifact after this sitting closes.
