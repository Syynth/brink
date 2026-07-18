# Stdlib spec — DRAFT (checkpoint, 2026-07-19)

Status: **the stdlib sitting in progress** — Phase A postures RULED,
domains 1–3 ruled-or-proposed as marked, domains 4–8 + closers open.
This document is the resumption point for any session (cloud
included): read the native-surface charter (§12–§13) first, then
this top-to-bottom; open items are marked ⏳, proposals awaiting the
maintainer's nod are marked 🔶.

## 1. Phase A — the postures (RULED)

1. **Errors: totality-first + faults; Result deferred-with-intent.**
   Design every verb total where honest; turn-terminating faults for
   true domain errors (the E078 lineage). Result/Option are PRESUMED
   future arrivals via #1090 (the maintainer expects them "sooner
   than later"); sentinel-returning verbs (`find`, `index_of` → -1)
   are their designated motivating martyrs, documented as such.
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
   static type language — `[T]` arrays, `[K: V]` maps (syntax
   mirrors literals; statically homogeneous; NOT user generics —
   #1090 candidate (b), promoted). Intrinsics carry checker-known
   polymorphic signatures (closed set); UFCS completion reads them.
   Docs display notation ⏳ (decide with the inventory).
5. **Namespace**: `std::` tree (`math`, `text`, `seq`, `rand`,
   `collections` — layout finalizes with the inventory); a curated
   ambient PRELUDE (marked per-verb below); `host::` mounts from the
   capability manifest (paths declared per entry); in-tree
   `extern fn` = same species, `story::`-side.

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
with arrays). `find` → index or -1 (Option martyr #1). Prelude:
`len contains char_at`; rest `std::text`.

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
- **Fn-value trio `map filter fold` — EAGER** (laziness explicitly
  deferred; chains price an intermediate). Their rows COMPOSE the
  callback's row (the #872 machinery): `a.map(f)` is
  pure·silent·total exactly when `f` is. (Ledger note: the trio is
  candidate (c) living inside the doctrine.)
- **Push/pull without laziness (RULED)**: (1) internal iteration —
  `each`/`for_each` — free today; (2) fused verbs (`filter_map`)
  for hot 2-stage chains; (3) **row-gated fusion**: the compiler
  may fuse intrinsic chains when callback rows prove PURITY (fusion
  only changes effect interleaving ⇒ purity makes it invisible) —
  the effect system licenses deforestation. True pull-iterators =
  future protocol; flows already serve coroutine-shaped needs.
- 🔶 **Mutation posture (proposed, awaiting nod)**: mutating verbs
  take `ref` first params; **UFCS auto-refs an lvalue receiver**
  (`inventory.push(sword);`; rvalue receiver = compile error).
  Naming: imperative = in-place (`sort push insert remove
  reverse`), past-participle = functional (`sorted reversed`).
- Verbs: `len first last index_of (Option martyr #2) contains
  slice(view) concat sort sort_by sorted reversed min max (empty ⇒
  fault) push pop insert remove each map filter fold filter_map`.
  Prelude: `len contains push`; rest `std::seq`.
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
- **The non-faulting read**: no Option yet, and maps have no honest
  in-band sentinel (unlike `find` → -1 — any V is a legal value).
  Proposal: `get_or(m, k, default)` — total by construction, no
  sentinel; plus `contains_key` for the test-then-index idiom
  (turns are single-threaded; no TOCTOU). A bare `get` is
  **Option martyr #3** — named hole, arrives with #1090, documented
  in its slot now.
- Verbs: `len contains_key get_or keys values remove clear`.
  `remove(ref m, k)` imperative/in-place per the mutation posture,
  total (removing an absent key is a no-op — deletion is
  idempotent; the faulting read covers "I expected it there").
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
inherited LIST operation — same runtime semantics underneath (ink
compat + oracle hold them); the native surface renames or retires
spellings only:

| ink inheritance | native | disposition |
|---|---|---|
| `LIST_COUNT` | `count` | keep (verb) |
| `LIST_ALL` | `all(Mood)` | keep — full-domain subset |
| `()` empty | `none(Mood)` | new spelling; empty-subset literal |
| `?` membership | `contains` | keep (verb; operator form ⏳ code-dialect sitting) |
| `+=` / `-=` | `add` / `remove` | keep both operator and verb; `ref` first param, in-place |
| `^` intersection | `intersect` | verb; operator form ⏳ |
| `LIST_MIN`/`MAX` | `first` / `last` | RENAMED — domain-order vocabulary, not numeric; empty subset ⇒ fault (matches seq `min`/`max` posture) |
| `LIST_VALUE` | `index_of`-shaped | ⏳ — needs the numeric-coupling question below |
| `LIST_RANGE` | `range` | keep, `range(Mood, a, b)` inclusive by domain order |
| `LIST_INVERT` | `invert` | keep — complement within the domain |
| `LIST_RANDOM` | moves to domain 6 | `rand::pick` accepts a flags subset (closed iterable set member) |

- `next`/`prev` step a **single-flag subset** by domain order.
  Off-the-edge yields `none` (ink's total, empty-result behavior —
  honest totality, kept); stepping a multi-flag or empty subset
  **faults** (domain error: "step what?"). This splits ink's
  silent-garbage cases from its honest one.
- **The numeric coupling** (⏳ needs a ruling): ink lets flags
  carry explicit numeric values and converts subsets↔ints. The
  clean native story is flags-as-symbols (ordinal queries via
  `index_of`-shaped verbs only); the numeric-values feature would
  then be ink-frozen (compat surface, not respelled). Recommend:
  freeze — the dossier shows no native-side demand, and enums with
  payloads now cover "symbol with data".
- Prelude: `contains count`; rest `std::flags`.

## 7. Domain 6 — random 🔶 (proposed)

- **THE effect answer — no new row dimension.** RNG state is a
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
  [0,1), `chance(p)` → bool, `pick(iterable)` (any closed-set
  iterable incl. flags subsets; empty ⇒ fault, matching
  `min`/`max`), `shuffle(ref a)` in-place + `shuffled(a)`
  functional (naming convention pair #1), `seed(n)`.
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
  (construction refuses empty/zero-weight tables at literal level —
  compile error where classifiable, construction fault otherwise:
  the E076/E078 split applied). `len`, iteration, and mutation ⏳ —
  v1 is construct-and-roll; the dossier shows no mutation demand.
- **Heap/priority queue — the humble form first.** Proposal: verbs
  over arrays, not a new type — `heap_push(ref a, x)`,
  `heap_pop(ref a)` (empty ⇒ fault), `heap_peek(a)` (empty ⇒
  fault), maintaining the invariant over an ordinary `[T]`
  (`std::collections`). Rationale: zero new value kinds, zero wire
  work, the Lua posture; comparison = the value-model ordering
  (min-heap; `sort`'s ordering, one ordering doctrine). If the
  ledger later shows shape-confusion incidents (heap-array indexed
  as if sorted), a sealed `Heap[T]` builtin is the designed
  upgrade path — recorded, not built.
- Anything further (deque, set-as-type) is **evidence-gated** —
  `std::collections` is the landing zone, the dossier is the gate.

## 9. Closers 🔶 (proposed dispositions)

1. **Anonymous records — KEEP, narrowed and renamed to their job:
   structural records.** With maps homogeneous and structs
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
   `@[effects(pure)]` ⊃ rng-freedom (§7). Holes' release policy ⏳
   — **deliberately left for the maintainer** (release-gating
   policy is an authorial-workflow value judgment, not derivable
   from the charter).
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

## 10. Remaining docket ⏳

- The three sitting-3 🔶s (UFCS auto-ref · naming convention ·
  eager trio) and now §§5–9 above await the maintainer's nod —
  **nothing in §§5–9 is ruled**; it is the between-sittings draft
  to react to.
- In-section ⏳s: tower mini-spec (§2b) · view-materialization
  ratio (§3b) · intrinsic display notation lives in §9.4 now ·
  flags numeric-coupling ruling (§6) · weighted-table mutation
  surface (§8) · anonymous-record native spelling (§9.1, owned by
  the code-dialect sitting) · holes' release policy (§9.2).
- Then: **Phase C** — the full inventory tables (every verb ×
  signature × row × prelude flag) appended here, and
  implementation sequencing (the numeric tower and effect-row
  extensions #1087/#1097 are compiler/runtime work that can pump
  BEFORE the parser exists; the surface-syntax-dependent parts
  wait on the prototype parser).

## 11. Session-resumption notes

Ruled context lives in: docs/native-surface-charter.md (§1–§13),
this doc, issues #1087/#1090/#1093/#1097, and the decision log.
§§1–4 are ruled as marked; **§§5–9 are proposals drafted between
sittings (2026-07-19, cloud session) and carry no rulings** — the
maintainer reacts top-to-bottom, then Phase C convenes. The
native-surface prototype parser is the season's next artifact
after this sitting closes.
