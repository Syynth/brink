# Phase C — findings (the pushback mandate)

Drafted 2026-07-19. The inventory (`phase-c-inventory.md`) is where the
2026-07-18 rulings compose verb-by-verb for the first time. This
document is the composition audit: every collision, incoherence, and
underspecification found while filling the tables, stated as a numbered
finding rather than silently resolved.

**Severity key.** **BLOCKING** = a ruling is needed before the marked
inventory cells are trustworthy (the table cannot honestly commit).
**NON-BLOCKING** = the tables can carry the open edge with a note; a
recommendation is offered but Phase C proceeds.

Nothing here is resolved by fiat. Each finding cites the colliding/silent
rulings by section, states why it matters, lays out options, and gives a
recommendation.

---

## Blocking findings (verbatim, for the final message)

### F0 — `sort_by`'s canonical signature contradicts the mutation posture — BLOCKING

**The collision.** §9.4 chooses `fn sort_by(a: [T], cmp: fn(T, T): int): [T]`
as the *standing display-notation exemplar* — a signature that takes
`a: [T]` (no `ref`) and **returns `[T]`**: the functional shape. But the
mutation posture (§4, decision-log "Mutation posture") rules
`sort`/`push`/`insert`/`remove`/`reverse` as **imperative = in-place**
(`ref` first param, `void` return), with past-participle (`sorted`,
`reversed`) as the functional twins. `sort_by` is imperative-form, so by
the naming convention it should be `fn sort_by(ref a: [T], cmp): void` —
the exact opposite of the exemplar the spec teaches from.

**Why it matters.** This is not cosmetic. It decides (a) whether
`sort_by` takes `ref` (and therefore whether `a.sort_by(cmp)` on an
*rvalue* is a compile error — the F-lattice); (b) the `void`-vs-`[T]`
return that a `let x = a.sort_by(cmp)` binding type-checks against; (c)
whether a functional `sorted_by` twin must exist (the past-participle
side of the pair is currently **missing** — there is no `sorted_by` in
any verb list). The spec's most-repeated teaching example encodes the
wrong answer to its own naming law.

**Options.**
1. **`sort_by` is in-place** (`ref a`, `void`), matching `sort`; add
   `sorted_by(a, cmp): [T]` as the functional twin; **change the §9.4
   exemplar** to a genuinely-functional verb (e.g.
   `fn map(a: [T], f: fn(T): U): [U]`) so the notation banner stops
   teaching a convention violation.
2. **`sort_by` is functional-only** (`a`, `[T]` — as the exemplar shows);
   accept that `_by` comparator verbs are exempt from the imperative =
   in-place rule; document the exemption; provide no in-place
   comparator sort (authors write `a = a.sort_by(cmp)` — but that is a
   *rebind*, not the in-place mutation `sort` advertises, so the pair
   `sort`/`sort_by` would then be inconsistent with each other).
3. Ship both `sort_by` (in-place) and `sorted_by` (functional) and pick
   the exemplar from elsewhere — the completionist option.

**Recommendation: Option 1.** The naming convention is a ruled standing
law with a stated pedagogy ("the verb carries the mutation signal");
`sort_by` reads as imperative and must behave so. The exemplar was
chosen for *notation shape* (nested `fn(T,T): int`), not for its
mutability — reselect a functional verb with an equally instructive
nested-fn param (`map`). This keeps the confusion lattice closed
(`let b = a.sort_by(c)` = unit type error, symmetric with
`let b = a.sort()`).

---

### F1 — does `string(x)` route through the `display` protocol? — BLOCKING

**The chain to check.** The 2026-07-13 conversion ruling: `string(x)`
accepts everything, "display form, **same as interpolation**, total,
never faults," fixed intrinsic return type `String`. The 2026-07-18
registry ruling: `display` is a protocol (`fn(T): string`, contract
pure·silent·total) that "feeds the §1.6 boundary" — and §1.6 names only
**interpolation** as the display boundary. Neither ruling states whether
the `string()` *conversion intrinsic* dispatches to a user's `display`
impl, or renders a structural default.

**Why it matters.** If `string(myStruct)` and `{myStruct}` diverge — one
using the user `display` impl, one the structural default — the
2026-07-13 "same as interpolation" guarantee is broken. If they agree,
the row must record that `string()` has a **protocol dependency on
`display`** (currently the inventory marks `string()` bare). It also
decides whether `string()` stays a fixed-return intrinsic or becomes a
protocol-dispatching verb.

**Does the totality chain hold?** Yes — *if* routing is confirmed. The
`display` contract is pure·silent·total, so `string()` composing it
stays pure·silent·total; the "never faults" guarantee survives because
the checker forbids a faulting `display` impl. The chain holds **only
because display impls are contract-bound** — this is the load-bearing
link the seed asked to verify, and it does hold.

**Options.** (a) both `string()` and interpolation dispatch to
`display`; (b) only interpolation dispatches, `string()` uses the
structural default; (c) `string()` dispatches, interpolation uses a
separate "narrative" path.

**Recommendation: (a).** The 2026-07-13 ruling explicitly bound
`string()` to interpolation's display form; honor it. Add `display` to
`string()`'s protocol-dep cell and to interpolation's. State in the spec
that both are the *same* display path, and that the pure·silent·total
contract is what preserves `string()`'s totality. **Blocking** because
the inventory's `string()` protocol-dep cell and the display protocol's
consumer list cannot be filled honestly until ruled.

---

### F3 — `chance(p)`'s domain posture is unruled — BLOCKING

**The hole.** §7 lists `chance(p) → bool` with no word on `p` outside
`[0, 1]`, and no word on `p = NaN`. Every other rand verb had its domain
nailed (`int` by refinement, `pick` by Option); `chance` was left bare.
The inventory's faults column for `chance` is therefore a guess.

**Why it matters.** `p` is the one rand parameter that takes an
arbitrary computed float, so out-of-domain values *will* arrive at
runtime. The choice sets precedent for scalar-probability domains
generally.

**Options.**
1. **Clamp-total**: `p ≤ 0` → always `false`, `p ≥ 1` → always `true`;
   `NaN` → `false` (or `true`); row stays `W:rng`, no fault. Matches the
   math domain's NaN-totality generosity and "probability" intuition.
2. **Fault** on `p ∉ [0,1]` or `NaN` (E078-style): row gains `F:parse`.
3. **Refinement**: an inhabited-`[0,1]` refinement like the range —
   over-engineering for a scalar with no parse-don't-validate
   amortization (there is no "N draws" to amortize a one-shot bool
   across).

**Recommendation: Option 1 (clamp-total), NaN → false.** The math
domain already commits to NaN-totality; `chance` is not an ordering
context, so §4b's NaN-fault fence does not reach it; clamping is the
unsurprising reading of a probability. Option 3's refinement machinery
buys nothing here (unlike the range, whose cost amortizes over many
draws). **Blocking** — the faults column cannot be filled without this.

---

### F6 — can an author shadow `display` (or `compare`/`next`)? E035 × UFCS × the registry — BLOCKING

**The collision.** Three ruled mechanisms meet with no defined
interaction: (1) prelude/stdlib names are **shadowable with the
E035-lineage *warning*** (permissive, §9.3); (2) UFCS resolves
`x.foo(y)` to a free function `foo`; (3) the registry protocols
(`display`/`compare`/`next`) are contract-bound methods that user types
*implement*. What happens if an author declares a free `fn display(x)`?

**Why it matters.** If UFCS `x.display()` resolves to the shadowed free
fn while interpolation `{x}` invokes the *protocol* `display`, the two
diverge — and interpolation's trustworthiness (the whole §1.6 boundary)
depends on `display` meaning one thing. Worse, if a shadowing free
`display` could *satisfy* the protocol, an author could install a
`display` that violates the pure·silent·total contract by the back door.

**Options.**
1. Protocol method names are **reserved** — shadowing `display`/`compare`
   /`next` is a **hard error** (not the E035 warning), because they name
   contract-bound compiler behaviors, not ordinary prelude verbs.
2. Shadowing is allowed (E035 warning) but the *protocol* dispatch is
   never affected — `{x}` always uses the registry impl; only free-call
   `display(x)` / UFCS `x.display()` see the shadow. (Divergence made
   explicit and legal.)
3. A free `fn display` *is* how you implement the protocol (the impl
   spelling is "declare the function") — in which case shadowing is
   implementing, and the contract check fires on it.

**Recommendation: Option 1 for v1**, revisited when the protocol impl
spelling is chosen (⏳ code-dialect sitting, §9.6). The registry names
exactly three method names; reserving three identifiers is cheap and
removes the divergence and the back-door contract violation at a stroke.
Option 3 is attractive but entangles this with the unresolved impl-
spelling question — don't couple them. **Blocking** because it governs
whether interpolation is trustworthy in the presence of author code, and
the E035 policy currently says "shadowable," which is the wrong answer
for these three names.

---

### F7 — ranges as first-class values: wire/equality/display unspecified — BLOCKING

**The hole.** `0..n` and `0..=n` "join the language" (§1.2) and are
consumed by `for`, `pick`, and `nonempty()`. But nothing specs whether a
range is a **storable Value**: its wire form, its equality
(`0..5 == 0..5`?), its display (`{0..5}` renders as?), whether `[range]`
/ `[K: range]` are legal, whether a range can be a `var`.

**Why it matters — and why it is genuinely blocking.** The flow-
suspension spec (decision-log 2026-07-16, FlowFrame) rules that **for-loop
iterators crossing an `await` spill into the frame record** as ordinary
`Value`s. A `for i in 0..n` loop that awaits mid-iteration therefore
*requires* the range (or its iterator) to have a durable wire form —
otherwise the save is impossible. This is not a "nice to have"; it is a
save-correctness dependency that the range ruling did not address. The
inventory cannot fill a range's row (signature/wire/equality/display) at
all right now.

**Options.**
1. Ranges are a **first-class Value kind** (min, max, inclusivity flag) —
   define wire form, structural equality, and a `display` (`"0..5"`);
   `[range]` legal. The inhabited-range refinement is then a refined
   view over this kind.
2. Ranges are **syntactic-only** — not storable, consumable solely by
   `for`/`pick`/`nonempty`; the iterator state that spills across a
   FlowFrame is a *synthesized* internal value (current index + bound),
   not a source-visible range. This bans `var r = 0..n` but keeps the
   Value taxonomy smaller.

**Recommendation: Option 1.** The FlowFrame requirement already forces a
durable iterator representation; promoting the range to a real Value
kind (small, three fields) is cheaper than maintaining a hidden
iterator-only encoding *plus* the syntactic-range forms, and it makes
`nonempty()`'s `Option<<inhabited range>>` return type a genuine value
rather than a phantom. Either way **a ruling is required** — the range's
row is empty until then. **Blocking.**

---

### F8 — the inhabited-range refinement in gradual mode — BLOCKING

**The hole.** `rand::int`'s parameter is the inhabited-range refinement:
literal bounds coerce free (strict, checker-proven), computed bounds go
through `(a..b).nonempty()`. But §7 is silent on **gradual mode**, where
the checker may not run the coercion. What happens when a
`types = gradual` project passes a possibly-empty computed range to
`rand::int` without `nonempty()`?

**Why it matters.** This is the language's *first* value refinement and
therefore the **template for every future refinement's gradual-mode
behavior**. Getting it wrong (or leaving it undefined) means `rand::int`
— advertised "total by type" — is actually partial in the default type
mode, silently.

**Options.**
1. **Runtime residual**: in gradual mode (or wherever the refinement
   isn't statically discharged), `rand::int` performs a runtime
   inhabited-check that **faults on empty** — mirroring the exact
   int()/E078 pattern (compile error strict / runtime fault gradual).
   The verb keeps a `faults` row entry that is *inert under strict*
   (parallel to §4b's mode-independent rows).
2. The refinement is **strict-only**; gradual projects get `Option` from
   `rand::int` (breaking the "total by type" property in gradual).
3. Gradual passes the range straight through and an empty range yields a
   defined value (e.g. always the low bound) — rejected: fabrication,
   violates the doctrine.

**Recommendation: Option 1.** It matches the established
strict-compile-error / gradual-runtime-fault split (the `int()` domain
ruling is the precedent), keeps `rand::int` honestly total in strict,
and makes the refinement's runtime residual explicit. Record it as the
**general refinement→gradual rule** so future refinements inherit it.
**Blocking** — it defines the semantics of the first refinement in the
default type mode.

---

### F10 — `for k, v in m`: exact lowering + mutation-during-iteration — BLOCKING

**The collision.** Two rulings meet: (i) maps `iterate` **by keys** via
the pull protocol (`next(ref Self): Option<K>` yields keys, §9.6); (ii)
`for k, v in m` desugars to "key-iteration + `let v = m[k]`" (§5, §9.1).
Where does the second binding come from under a pull protocol that
yields *one* value?

**The lowering (proposed, needs ratifying).**
```
for k, v in m { <body> }
⟶  for k in m {              // pull: next(ref iter) → Option<K>
       let v = m[k]          // read-index; total because k came from m
       <body>
   }
```
The second binding is **synthesized in the loop body**, not produced by
the protocol. `m[k]` normally read-faults on a missing key, but here `k`
is drawn from `m`'s own keys, so it is total *by construction* — **as
long as the map is not structurally mutated mid-loop.**

**Why it is blocking — the mutation hazard.** The pull protocol iterates
the **live** map. If `<body>` does `remove(ref m, k2)` (or `m[k3] = v`,
or `clear`), a later `m[k]` can fault (key removed) or the key stream can
skip/repeat. The desugar's "total by construction" claim **silently
assumes no concurrent structural modification** — an assumption nothing
in the spec states or enforces. This is exactly the "silent data drop /
silent fault" class the project's own rules say to flag on sight.

**Options.**
1. **Snapshot keys eagerly**: `for k in m` lowers over `m.keys()` (an
   insertion-order snapshot array), not the live pull iterator — then
   mid-loop mutation is safe (iterates the snapshot; `m[k]` may still
   fault if the body removed `k` itself → define that as a fault or
   guard with `get`). Costs one array allocation per map loop.
2. **Live pull + fault on structural modification**: a modification
   counter; mutating `m` during its own iteration is a turn-terminating
   fault (the fail-fast option).
3. **Live pull, undefined on mutation** — rejected: undefined behavior
   is the banned outcome.

**Recommendation: Option 1 (snapshot keys) for maps**, with `let v = m[k]`
replaced by `let v = m.get(k)` *only if* self-removal during iteration is
to be tolerated — otherwise keep `m[k]` and rule self-key-removal-during-
iteration a fault. Snapshotting is the least-surprising, matches
`keys()`'s already-eager insertion-order semantics, and sidesteps the
pull-protocol liveness hazard for the one built-in map consumer. Note
this makes maps' `for` desugar *not* use the generic pull protocol — a
deliberate exception worth stating (the protocol still governs user
iterables and `for k in userIter`). **Blocking** — the exact lowering
and the mutation semantics are both required, and the naive reading
hides a fault.

---

## Non-blocking findings (tables carry the open edge)

### F2 — min/max over `[float]`: two absence-ish channels — NON-BLOCKING (coherent)

`min`/`max` over `[float]` carry **both** `Option<float>` (empty → none)
**and** `F:float` (dev-fault on NaN / prod pinned order). Are two
"no-good-answer" channels coherent? **Yes** — they are orthogonal:
emptiness is *absence* (the world had no elements → Option), NaN is a
*bug* (→ fault in dev) or a placed element (→ prod pinned order). The
row honestly carries both. One prod edge worth documenting:
`min([NaN])` in prod returns `Some(NaN)` (NaN sorts greatest, is the sole
element) — a `Some` wrapping NaN, not fabrication (the NaN was in the
data). **Recommendation:** keep; document the two channels side-by-side
in the verb doc (the teaching burden is real but the semantics are
clean).

### F4 — `rand::float()` vs the `float(x)` conversion intrinsic — NON-BLOCKING

Two verbs named `float`: `rand::float()` (nullary, → [0,1), `W:rng`) and
the global conversion `float(x)` (unary, pure, 2026-07-13). Disambiguated
by namespace + arity: bare `float()` with no args is an arity error on
the conversion intrinsic; `rand::float()` must be namespace-qualified
(rand has no prelude). **Recommendation:** confirm `rand::float`'s row is
`W:rng` (done in the inventory); require it always be written
`rand::float()`. Mild ergonomic collision; note it, don't rule on it.

### F5 — the removal matrix: `pop`/`remove`×3, and `remove`'s return type — NON-BLOCKING (one sub-item blocking-lite)

**F5a (teaching).** Removal-shaped operations now span four postures:
seq `remove(ref a, i)` (OOB → **fault**, index = a claim), map
`remove(ref m, k)` (**idempotent-total**, deletion = a wish), flags
`remove(ref s, m)` (**idempotent-total**, subtract), seq `pop(ref a)`
(→ **Option**, empty = absence). The maintainer-attention note already
flags the triple-`remove`; adding `pop` makes it a 4-way matrix. Coherent
under the doctrine (index/claim vs deletion/wish vs pop/ask), but the
teaching load is real. **Recommendation:** ship a "removal cheat-sheet"
in the docs; the divergence is *chosen*, keep it.

**F5b (blocking-lite underspecification).** Does seq `remove(ref a, i)`
**return the removed element** or `void`? `pop` returns `Option<T>`
(the element); map/flags `remove` return `void`. If seq `remove` returns
the element, its return type diverges from the other `remove`s (which is
fine — intrinsic overloading) but is unspecified. **Recommendation:** seq
`remove(ref a, i): T` returns the removed element (it faults on OOB, so
there is always an element to return — no Option needed); map/flags
`remove` stay `void` (idempotent, nothing meaningful to return). State it.

### F9 — `filter_map` is in the pure-required trio — NON-BLOCKING (confirm)

`filter_map(f: fn(T): Option<U>)` is listed with the trio and named a
fused hot-path verb (§4). Its callback returning `Option<U>` is
orthogonal to purity (Option is *data*, not an effect). **Recommendation:**
confirm **yes, `filter_map` is pure-required** like `map`/`filter`/`fold`.
Note the distinction from `map(f: fn(T): Option<U>): [Option<U>]` (which
*keeps* the nones as elements) — `filter_map` drops them. Both legal;
teach the difference.

### F11 — `min`/`max` span three overload homes with two postures — NON-BLOCKING

`min`/`max` resolve to: math scalar-pair (`min(a, b)`, total, **prelude**),
seq array (`min(xs)`, **Option**, `std::seq`, not prelude), and tower
componentwise (§2b). Same name, **total in one home, Option in another**.
Mechanically fine (checker-known intrinsic overloading, resolved by
arg type/arity), but an author sees `min` behave differently by arity.
**Recommendation:** keep; ensure the checker's overload resolution and
UFCS completion surface **all three** signatures under the one name, so
the arity/posture split is visible at the call site.

### F12 — a None-rendering interpolation still `emits` — NON-BLOCKING

The §1.6 display boundary renders a final-None interpolation as nothing.
Under #1087, does a def containing `{maybeNone}` still count as an
emitter? **Yes** — it is still a content line, merely one that may render
empty; emits is a static, conservative property. **Recommendation:**
confirm emits-inference treats interpolation-of-Option as `emits`
regardless of whether it renders empty at runtime; the traceability rider
(transcript records the None-render) is separate.

### F13 — seq `slice` OOB posture by parity — NON-BLOCKING

Text `char_at`/`slice` OOB **fault** (§3, explicit). Seq `slice(view)`
(§4) does not state its OOB behavior. **Recommendation:** by the one-
indexing-contract principle ("OOB indexing is a bug"), seq `slice` OOB
**faults** too (inventory marks `F:oob`). Also underspecified for both:
`slice(a, start, end)` with `start > end` — recommend a fault (malformed
range) or defined-empty; pick one. Note the open edge.

### F14 — `sort_by` does **not** inherit the `[float]` unconditional fault — NON-BLOCKING

`sort`/`sorted`/`min`/`max` carry `F:float` because they use the
intrinsic order. `sort_by` uses a **user comparator returning int** — NaN
never reaches the ordering machinery as a comparison *result*; the
comparator owns the order. So `sort_by` over `[float]` does **not** carry
the §4b `[float]` fault; its faults come from `⊕cmp` (the comparator may
fault) ∪ detected-inconsistency (the implementation may fault on a non-
total-order comparator, guarantee floor "some permutation, never worse").
**Recommendation:** state this explicitly — `sort_by`'s row is
`⊕cmp + inconsistency`, distinct from `sort`'s `F:float`. (Depends on F0
for the rest of the signature.)

### F15 — user `compare` vs built-in structural equality — NON-BLOCKING (already owed)

§9.6 already flags this owed: `compare == 0` need not imply structural
`==`. The asymmetry is sharper than it looks: **equality is a built-in**
(content comparison, 2026-07-18, insertion-order-insensitive) while
**ordering is protocol-gated** (`compare`). So `sort`/`min`/`max`/heap
over a struct with a user `compare` can place two elements as "equal by
compare" that are `!=` by structural equality. **Recommendation:** rule
the coherence line the spec already owes — recommend "`compare` and `==`
are independent; verbs that need equality use `==`, verbs that need order
use `compare`; the two coincide on the built-in scalar/array orders by
construction but user impls are not required to reconcile them." Tables
note the open edge.

### F16 — the `as`-binding Option-unwrap construct is unspelled — NON-BLOCKING (surface)

§8 writes `while heap_pop(ref open) as node { … }` and the flow-suspension
spec writes `while await cond`. The `EXPR as NAME` binding that unwraps an
`Option` in `if`/`while` position is **used but never designed** in any
ruled grammar. It is the primary ergonomic consumer of every Option-
returning verb (`pop`, `get`, `first`, `heap_pop`, …). **Recommendation:**
flag as a **parser-dependent surface item** the code-dialect sitting must
design (alongside `or` and the display boundary); the sequencing doc puts
it in wave B. Non-blocking for the inventory (verbs return `Option`
regardless of consumption spelling) but the ergonomics are hostage to it.

### F17 — `Weighted { … }` duplicate weights vs map duplicate keys — NON-BLOCKING (strong flag, feeds #1103)

The "one initializer grammar, per-type meaning" ruling (§9.1) puts
`Weighted { 3: sword, 1: shield }` and `Map { k: v }` on the **same
brace-colon grammar**. But their duplicate-key policies must **diverge**:
`Weighted { 3: sword, 3: shield }` is *legal and meaningful* (two weight-3
items — a multiset), whereas `Map { 3: a, 3: b }` is a duplicate-key
**error** (E076/E084 lineage). Same syntax, opposite duplicate rule.
**Why it matters:** this is direct evidence for #1103 — "per-type meaning"
already diverges on a semantic (not just structural) axis, which a
*construction protocol* (per-type `Add`-dispatch, the C# lineage) would
model cleanly and grammar-dispatch would have to special-case.
**Recommendation:** state Weighted's braces are a **multiset** (dup
weights legal), explicitly distinct from Map's key-set; file the duplicate-
policy divergence as an input to #1103. Non-blocking for the tables (the
inventory records it) but it sharpens the #1103 question.

### F18 — `pick(range)` → Option vs `rand::int(range)` → total — NON-BLOCKING

Two APIs for "random int in a range": `rand::int(inhabited-range)` is
**total** (refinement amortizes the check across N draws), while
`pick(0..n)` returns **Option** (empty → none, per-call). The parallel is
confusing — same conceptual operation, two totality contracts.
**Recommendation:** keep (they encode different ergonomic bets — `int`
for the "I validated the range once, now draw many" path, `pick` for the
"draw once from whatever" path); document the choice. Consider (evidence-
gated, not now) a `pick` overload accepting an inhabited range for a
total return.

### F19 — `or` typing and chaining — NON-BLOCKING (substrate)

The `x or default` coalescing spelling needs a stated type rule. Is it
`(Option<T>, T) → T` only, or also `(Option<T>, Option<T>) → Option<T>`
for chaining (`a.get(k) or a.get(k2) or default`)? **Recommendation:**
both overloads — the two-Option form keeps optionality for chaining, the
Option-then-value form collapses to `T`; left-associative. This belongs
in the Option-package substrate spec (wave A), not a per-verb row, but
the rows lean on it. Flag so it is not forgotten.

### F21 — `index_of` diverges: seq/text Option vs flags int-or-fault — NON-BLOCKING

`index_of` returns `Option<int>` in seq/text (element may be absent) but
`int` in flags (a single flag always has a domain position; multi/empty
subset **faults**). Same name, different return type *and* fault posture.
Coherent under the doctrine (a flag's position always exists; an array
element's may not), but joins the same-name-divergent-posture roster
(`remove`, `min`/`max`, `first`/`last`). **Recommendation:** keep; add to
the divergence roster the maintainer-attention note tracks.

### F22 — `contains`/`index_of`/`contains_value` depend on `eq`, not `compare` — NON-BLOCKING

Membership/search verbs need **equality**, not ordering. Since equality
is the built-in structural comparison (not a registry protocol), these
verbs' protocol-dep is `eq` (built-in), and they work over `[struct]`
**without** a `compare` impl (but `sort`/`min`/`max` over `[struct]`
**require** `compare`). **Recommendation:** state the split — search =
built-in equality, order = `compare` protocol — so authors know a struct
array is searchable but not sortable until it implements `compare`.

### F23 — `div_floor`/`mod_floor` (and `/`,`%`) zero-divisor posture — NON-BLOCKING

`div_floor(a, 0)` / `mod_floor(a, 0)` — fault or defined? Division by
zero is unspecified in §2. **Recommendation:** fault (`F:div0`, E078
lineage — a zero divisor is a bug, consistent with the absence/bug split);
confirm the frozen `/`/`%` operators' existing ink behavior and whether
the native grid verbs match or diverge (two-surface). The inventory marks
`F:div0` provisionally. Note also `sqrt(-1.0)` = NaN (ruled, not a fault)
is the *deliberate* contrast — arithmetic domain errors are NaN-total,
but a zero divisor has no NaN to produce for `int` results, forcing the
fault question.

### F24 — the numeric tower is un-tabbable until its mini-spec — NON-BLOCKING (already ⏳)

§2b defers value kinds, wire, codecs, marshal legs, NaN/equality
composition, majorness/handedness, and save posture to an owed mini-spec.
The inventory's tower rows (`dot`/`cross`/operators) are therefore
**partial**. **Recommendation:** Phase C tables carry the tower as
explicitly-partial; the mini-spec is a prerequisite for the tower's full
inventory and for its `display`/equality protocol rows. Sequencing doc
lists it as a wave-A substrate item (compiler-known value kinds).

### F25 — `repeat(s, n)` / negative-count postures — NON-BLOCKING

`repeat(s, n)` with `n < 0`, and `insert(ref a, i, x)` / `range(Mood, a,
b)` with reversed or negative arguments, have no stated posture.
**Recommendation:** `repeat` with `n ≤ 0` → empty string (total, the
generous reading); reversed `range(Mood, b, a)` where `b > a` in domain
order → empty subset or fault (pick one — recommend empty subset, matches
`none`). Minor; note the edges.

### F26 — is in-place `reverse` shipped? — NON-BLOCKING

The §4 verb list has `reversed` (functional) but **not** `reverse` (in-
place), yet the mutation-posture naming section lists `reverse` as an
example of an imperative in-place verb. By pair-completeness (`sort`/
`sorted`) the in-place `reverse` should exist. **Recommendation:** ship
`reverse(ref a): void` as `reversed`'s in-place twin; the inventory
includes it provisionally. (If deliberately omitted, state why — but the
naming section already names it, so omission looks like a list gap.)

### F27 — `concat` doesn't fit the imperative/past-participle axis — NON-BLOCKING

`concat(a, b): [T]` is functional (returns new) but its name is neither
imperative-in-place nor past-participle. So are `first`, `last`, `slice`,
`min`, `max` — query/functional verbs with **no mutation twin**. The
naming convention (imperative = in-place, past-participle = functional)
governs only verbs that *have* a mutation option (`sort`/`sorted`).
**Recommendation:** state the convention's scope explicitly — it is a
**pair-disambiguation rule**, not a claim that every imperative-form verb
mutates. Otherwise a cold reader may expect `concat` to be in-place.

---

## Roll-up

**Blocking (7):** F0 (sort_by signature vs mutation posture), F1
(string() ↔ display protocol routing), F3 (chance(p) domain), F6
(shadowing protocol names), F7 (ranges as first-class values / FlowFrame
wire requirement), F8 (inhabited-range in gradual mode), F10 (for-k-v
lowering + mutation-during-iteration).

**Non-blocking (20):** F2, F4, F5(a/b), F9, F11, F12, F13, F14, F15, F16,
F17, F18, F19, F21, F22, F23, F24, F25, F26, F27.

The blocking set clusters in three places: **the mutation/return-shape
axis** (F0, F5b), **the Option/display/protocol composition** (F1, F6),
and **the refinement/range/iteration substrate** (F7, F8, F10). None of
the seven is a re-litigation of a 2026-07-18 ruling — each is a gap the
rulings left where they compose. That is the expected shape of a first
verb-by-verb pass.
