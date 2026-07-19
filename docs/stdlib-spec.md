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
   choice text vs the deliberate `* []`) ⏳.

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
its machinery). Mini-spec owed ⏳: value kinds, wire, codecs,
marshal legs, NaN/equality composition, majorness/handedness per
glam, save posture.

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
  sort sort_by sorted reversed min max push pop insert remove
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

## 5. Domain 4 — maps 🔶 (proposed, drafted between sittings)

- Type `[K: V]` (doctrine §1.4); literal `Map { k: v }` (Phase A
  §1.3). Statically homogeneous both legs. **Key domain = the
  scalar map-key set + unit enum variants** (charter §13.1); a
  non-scalar key is the E076 lineage at compile time where
  classifiable.
- Indexing contract stands as ruled (#856): `m[k]` read faults on
  a missing key; `m[k] = v` inserts. No `insert` verb — write-index
  IS insertion (one spelling per concept).
- **The non-faulting read (updated per the 2026-07-18 Option
  ruling)**: `get(m, k)` → `Option[V]` — martyr #3, redeemed
  before it was ever martyred. `m.get(k) or default` covers the
  with-default idiom, so no `get_or` verb ships (the `or`
  spelling subsumes it — one spelling per concept); `contains_key`
  stays for the pure membership test. The faulting `m[k]` remains
  the "I expect it there" read (#856 unchanged).
- Verbs: `len contains_key get keys values remove clear`.
  `remove(ref m, k)` imperative/in-place per the mutation posture,
  total (removing an absent key is a no-op — deletion is
  idempotent; the faulting read covers "I expected it there").
  `clear(ref m)` in-place, total.
  `keys`/`values` → eager array snapshots in insertion order
  (iteration order is already insertion order; equality alone
  ignores it, per the 2026-07-18 ruling).
- **`entries` is gated on the anonymous-record closer** (§9.1): a
  pair needs a shape; if anonymous records survive, `entries(m)` →
  `[{ key, value }]` and struct patterns destructure it in `for`.
  Not shipped before that ruling.
- `for k in m` iterates keys (ruled); `m.keys()` is the same set
  reified. Prelude: `len` only (already ambient); the rest
  `std::map`.

## 6. Domain 5 — flags 🔶 (proposed): the LIST-op audit

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
| `LIST_VALUE` | `index_of`-shaped | ⏳ — needs the numeric-coupling question below |
| `LIST_RANGE` | `range` | keep, `range(Mood, a, b)` inclusive by domain order |
| `LIST_INVERT` | `invert` | keep — complement within the domain |
| `LIST_RANDOM` | moves to domain 6 | ✚ `rand::pick` accepts a flags subset (closed iterable set member); empty → `Option` none (dynamic-content absence — §7); frozen ink op untouched |

- `next`/`prev` (✚) step a **single-flag subset** by domain order,
  returning `Option` — off-the-edge is expected absence (`none`,
  the typed version of ink's honest empty). Stepping a multi-flag
  or empty subset **faults** — a malformed question is a bug, not
  absence (the doctrine's two halves in one verb). Ink's own
  `+1`/`-1` stepping stays frozen.
- **The numeric coupling** (⏳ needs a ruling): ink lets flags
  carry explicit numeric values and converts subsets↔ints. The
  clean native story is flags-as-symbols (ordinal queries via
  `index_of`-shaped verbs only); the numeric-values feature would
  then be ink-frozen (compat surface, not respelled). Recommend:
  freeze — the dossier shows no native-side demand, and enums with
  payloads now cover "symbol with data".
- Prelude: `contains count`; rest `std::flags`.

## 7. Domain 6 — random 🔶 (proposed)

- **Proposed effect answer — no new row dimension.** RNG state is a
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
  deliberate acts, namespaced): `int(range)` (range value —
  `0..10` / `0..=9` join as first-class arguments), `float()` →
  [0,1), `chance(p)` → bool, `pick(iterable)` →
  `Option` (any closed-set iterable incl. flags subsets; empty →
  `none` — dynamic-content absence, matching the flipped
  `min`/`max`), `shuffle(ref a)` in-place + `shuffled(a)`
  functional (the ruled §4 naming convention exercised again),
  `seed(n)`. `int` on an empty range (`0..0`, `5..5`) ⇒ **fault**
  🔶 — proposed cut: a range is a shape you wrote, so an empty one
  is a logic bug (and Option-wrapping every `rand::int(0..10)`
  would be purgatory); `pick`'s iterable is dynamic content, so
  its emptiness is absence. Confirm or flatten.
- Heritage: ink's `RANDOM(min, max)` / `SEED_RANDOM` stay
  ink-frozen spellings of the same cell — one RNG, two surfaces,
  no drift.

## 8. Domain 7 — collections+ 🔶 (proposed)

- **Weighted tables — the dossier's evidenced structure.** A
  parameterized builtin `Weighted[T]`; construction reuses the map
  literal shape with weights as keys: `Weighted { 3: sword,
  1: shield }` (grammar `weight: value` as chartered; weights =
  positive ints v1). One draw verb: `rand::roll(w)` → T — lives in
  domain 6's namespace because its row writes the rng cell; total
  (construction refuses empty/zero/negative-weight tables — the
  E078-style split: compile error where statically classifiable,
  construction fault otherwise; the compile diagnostic is a NEW
  code owed by Phase C, not an existing one). `len`, iteration,
  and mutation ⏳ — v1 is construct-and-roll; the dossier shows no
  mutation demand.
- **Heap/priority queue — the humble form first.** Proposal: verbs
  over arrays, not a new type — `heap_push(ref a, x)`,
  `heap_pop(ref a)` → `Option`, `heap_peek(a)` → `Option` (empty
  is absence, per the 2026-07-18 doctrine — and `while
  heap_pop(ref open) as node { … }` is the natural drain loop),
  maintaining the invariant over an ordinary `[T]`
  (`std::collections`). Rationale: zero new value kinds, zero wire
  work, the Lua posture; min-heap. ⏳ **A total-ordering doctrine
  is OWED, shared by `sort`/`sort_by`/heap** — the value model has
  none today (map iteration order is itself an open ruling, and
  NaN breaks float ordering: NaN-bearing `[float]` under
  sort/heap is currently undefined — the doctrine must say). If the
  ledger later shows shape-confusion incidents (heap-array indexed
  as if sorted), a sealed `Heap[T]` builtin is the designed
  upgrade path — recorded, not built.
- Anything further (deque, set-as-type) is **evidence-gated** —
  `std::collections` is the landing zone, the dossier is the gate.

## 9. Closers 🔶 (proposed dispositions)

1. **Anonymous records — proposed: keep, narrowed and renamed to
   their job: structural records.** With maps homogeneous and structs
   declared, the anonymous record `#{ x: 1 }` (native spelling ⏳ —
   likely bare `{ x: 1 }` where unambiguous, code-dialect sitting
   owns the final call) survives as exactly two things: (a) the
   **multi-return vehicle** (`fn stats(a): { min: int, max: int }`
   — no tuples, by design), destructured by the ruled struct
   patterns in `let`; (b) **`entries()`' element shape** (§5).
   Structural typing, width-exact, no declaration. If (a)/(b) were
   instead rejected, tuples would be back on the table — one or
   the other, and records are already load-bearing in the value
   model.
2. **Assertion spellings — final form** `@[effects(…)]` with args
   from `{pure, silent, total}`, any subset, comma-joined;
   exceedance-only errors (asserting less than reality is legal).
   `@[effects(pure)]` ⊃ rng-freedom (§7). Doc-sync owed: the
   effects spec and #1087 still show the older `#@effects(…)`
   spelling — supersession note there when this nods. Holes'
   release policy ⏳ — **deliberately left for the maintainer**
   (release-gating policy is an authorial-workflow value judgment,
   not derivable from the charter).
3. **Prelude — final list assembled from the per-domain marks:**
   entire math kit incl. trig (§2's generous ruling) · `len
   contains char_at` (text) · `len contains push` (seq) · `len`
   (maps) · `contains count` (flags) · nothing from rand/
   collections. Name-collision policy: prelude names are
   **shadowable with the E035-lineage warning** (stdlib-slice-1
   posture carries over).
4. **Docs display notation for intrinsic signatures**: the
   pseudo-generic letter form — `fn sort_by(a: [T], cmp:
   fn(T, T): int): [T]` — with a standing banner: *display
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
   - `compare` — `fn(T, T): int`, row ⊆ pure·silent·total; owns
     the ⏳ total-ordering doctrine (§8) incl. the NaN decision,
     made once. Coherence edge to state explicitly: user `compare`
     vs ruled structural equality (`compare == 0` need not imply
     `==`).
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
- In-section ⏳s: tower mini-spec (§2b) · view-materialization
  ratio (§3b) · intrinsic display notation lives in §9.4 now ·
  flags numeric-coupling ruling (§6) · rand::int empty-range
  fault-vs-Option confirmation (§7) · total-ordering doctrine
  incl. NaN, now homed in `compare` (§8/§9.6) · weighted-table
  mutation surface (§8) · anonymous-record native spelling (§9.1,
  owned by the code-dialect sitting) · holes' release policy
  (§9.2) · protocol implementation spelling (§9.6).
- Maintainer-attention note: `remove` now names three verbs with
  divergent postures — seq remove-by-index (OOB ⇒ fault, the
  indexing contract), map remove-by-key (idempotent-total), flags
  subtract (idempotent-total). Legal under intrinsic overloading;
  flagged so the divergence is chosen, not accidental.
- Then: **Phase C** — the full inventory tables (every verb ×
  signature × row × prelude flag) appended here, and
  implementation sequencing (the numeric tower and effect-row
  extensions #1087/#1097 are compiler/runtime work that can pump
  BEFORE the parser exists; the surface-syntax-dependent parts
  wait on the prototype parser).

## 11. Session-resumption notes

Ruled context lives in: docs/native-surface-charter.md (§1–§13),
this doc, issues #1087/#1090/#1093/#1097/#905 (FSM sidebar), the
PR #1100 conversation thread, and the decision log. §§1–4 are
ruled as marked (Option package + absence flips ruled
2026-07-18 in-conversation); **§§5–9 are proposals updated to
conform, awaiting the nod** — the maintainer reacts top-to-bottom,
then Phase C convenes. The native-surface prototype parser is the
season's next artifact after this sitting closes.
