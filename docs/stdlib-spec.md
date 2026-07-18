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

## 5. Remaining docket ⏳

- **Domain 4 — maps**: verb set (`keys values entries remove len
  contains_key`…), literal `Map { k: v }` details, `[K: V]` typing
  edges.
- **Domain 5 — flags**: verb surface post-rename (`count next prev
  all none`…, LIST-op inheritance audit).
- **Domain 6 — random**: THE effect question (rng draws write
  runtime RNG state — rows say what exactly?), seeding/determinism
  posture, shuffle/pick/weighted-roll relationship to domain 7.
- **Domain 7 — collections+**: heap/priority queue, weighted
  tables (dossier-evidenced; initializer grammar `weight: value`).
- **Closers**: anonymous-record fate (vs finished maps+structs);
  assertion spellings final form (`@[effects]` args, holes'
  release policy); prelude final list; docs display notation for
  intrinsic signatures; std:: tree final layout.
- Then: **Phase C** — the full inventory tables (every verb ×
  signature × row × prelude flag) appended here, and implementation
  sequencing (which waves build what — note the numeric tower and
  effect-row extensions #1087/#1097 are compiler/runtime work that
  can pump BEFORE the parser exists; the surface-syntax-dependent
  parts wait on the prototype parser).

## 6. Session-resumption notes

Ruled context lives in: docs/native-surface-charter.md (§1–§13),
this doc, issues #1087/#1090/#1093/#1097, and the decision log.
The three 🔶 items above are the only un-nodded proposals. The
native-surface prototype parser is the season's next artifact after
this sitting closes.
