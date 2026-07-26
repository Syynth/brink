# Collections: Arrays, Maps, and Ranges

The Last Light inn keeps its accounts the way it keeps its guests — in
order, by name, and without losing anybody:

```ink
~ temp tab = #[4, 7, 2, 5]
~ temp rooms = #{"Mira": 3, "Old Tom": 1}
~ temp owed = 0
~ {
    for coins in tab {
        owed = owed + coins
    }
}
The innkeeper runs a finger down the ledger: {len(tab)} nights on the tab, {owed} coins in all.
For the magistrate's copy, in order: {sorted(tab)}.
Mira keeps room {rooms["Mira"]}; Old Tom, room {rooms["Old Tom"]}.
-> END
```

```text
The innkeeper runs a finger down the ledger: 4 nights on the tab, 18 coins in all.
For the magistrate's copy, in order: [2, 4, 5, 7].
Mira keeps room 3; Old Tom, room 1.
```

Two collections carry that scene: the tab is an **array** — a sequence,
ordered by position — and the register is a **map** — a lookup, ordered by
when each guest signed in. The third kind this chapter covers, the
**range**, is a span of integers (`0..10`, `1..=6`) held as a value. All
three are *values* in the full sense the
[Values & Types](types.md) chapter established: assigning one copies it,
passing one to a function copies it, and no mutation ever reaches back
through a copy. What that buys you — and what each kind's contracts are
when you read, write, grow, shrink, and sort them — is the subject of this
chapter.

> **Current spelling** — examples in this chapter compile in today's brink
> dialect: collection literals carry the `#[…]`/`#{…}` sigils, type
> ascriptions spell `array<T>`/`map<K, V>`, mutating verbs are free calls
> (`push(tab, 5)`), and function values are `#fn(name)` references. The
> ruled native `.brink` spellings — bare `[…]` literals, `Map { k: v }`
> construction, `[T]`/`[K: V]` type notation, method-position calls with
> auto-ref (`tab.push(5)`), and `|a, b| …` lambdas — arrive with the
> native frontend, and this chapter's examples will be respelled then.

## The three kinds

- An **array** answers "what's at position `i`?" Elements share one type;
  order is the author's, kept exactly.
- A **map** answers "what's filed under `k`?" Keys are scalars (`int`,
  `string`, `bool`); entries keep the order they were inserted in.
- A **range** answers "the integers from here to there" — `0..n`
  (half-open) or `0..=n` (inclusive) — without materializing them. It
  behaves like a read-only array of its integers: it has a `len`, indexes,
  iterates, and compares.

There is a fourth collection-shaped kind in the language — `flags`, an
ordered domain of named symbols with subset-valued variables (ink's `LIST`
lineage). It is a *domain* first and a collection second, so it is taught
with enums and structs in that chapter of the book's reorganization, not
here.

## Writing one down

The dialect's literal forms are **sigils** — a leading `#` marks them as
extension syntax before the parser has to decide anything else:

- **Array**: `#[expr, expr, …]` — trailing comma allowed, `#[]` for empty.
- **Map**: `#{key: expr, key: expr, …}` — trailing comma allowed, `#{}`
  for empty.

Nesting is unrestricted — `#[#{a: 1}, #{a: 2}]` is a two-element array of
one-entry maps, and a ragged 2-D grid is exactly what it looks like:

```ink
VAR grid = #[#[1, 2], #[3]]
Row lengths: {len(grid[0])} and {len(grid[1])}.
-> END
```

```text
Row lengths: 2 and 1.
```

As that example shows, a collection literal is legal as a `VAR`/`CONST`
declaration default — nesting included — provided everything inside it is
a compile-time constant (literals and `CONST` references fold; a
declaration default is data, not code). The rule is enforced as real
errors, never silent nulls: a non-constant element or map value inside
the literal is `E077`, a map key that doesn't fold to a scalar is
`E076`, and a default that computes at the top level (a function call, a
reference to another `VAR`) is the general `E083` every declaration
obeys.

**Literals live in expression position only** — `~` lines, block
statements, call arguments, condition expressions. You cannot write
`Loot: #[10, 20].` as narrative text, and the restriction is forced by
ink's own grammar, not taste: in prose, `#` opens a **tag**
(`Some text # a_tag`), and tags legally contain `{}` interpolation, so
`#{…}` mid-prose is genuinely ambiguous with tag syntax. Expression
position has no such clash — `#` can never begin an ordinary ink
expression there — so that's the honest scope of "collision-free." The
idiom that follows: **compute first, narrate second.** Build the
collection in logic, then interpolate the variable — interpolation was
never restricted, because a variable reference isn't literal syntax:

```ink
VAR arr = 0

~ {
    arr = #[]
    push(arr, 1)
    push(arr, 2)
    push(arr, 3)
}

Arr is {arr}.
-> END
```

```text
Arr is [1, 2, 3].
```

(Under `strict-ink` — a project that never opted into the dialect — every
form on this page is rejected whole with a targeted `E051`: "brink
extension used under strict-ink dialect." Parse never fails; analysis
refuses. See [Enabling the Dialect](enabling.md).)

> **A bare `#` in prose swallows the rest of the line.** The tag grammar
> that forces the expression-position rule has a sharper edge worth
> knowing: because `#` opens a tag anywhere in prose, a literal `#` in
> narrative text — a hashtag, a shorthand for "number" — silently turns
> everything to the next `#` or end of line into tag data. `This costs
> # 5 dollars.` prints `This costs` and files `5 dollars.` as a tag. This
> is stock ink behavior, byte-identical to the reference implementation
> (verified against inklecate, issue #858) — not a brink bug, but a trap
> the sigil forms sit next to. Relatedly, trailing whitespace on a
> printed line — including whitespace after a final interpolation — is
> stripped before output, also matching the reference exactly.

## Typing a collection

Collections are statically homogeneous: one element type per array, one
key type and one value type per map. In an annotation the spellings are
`array<T>` and `map<K, V>`; almost everywhere, though, you write nothing
and inference reads the literal:

- `#[1, 2, 3]` is an `array<int>`.
- `#[1, 2.5]` is an `array<float>` — the one implicit numeric promotion
  (`int → float`, see [Values & Types](types.md)) joins elements before
  homogeneity is judged.
- `#{"Mira": 3}` is a `map<string, int>`.

Elements that *can't* unify make the collection's type `Conflicted`, and
under strict types (the brink dialect's default) the binding holding it
fails with `E066` — "`pantry`'s temp `tray` is Conflicted under strict
types — its uses disagree on its type":

```ink,error(E066)
-> pantry

=== pantry ===
~ temp tray = #[3, "brandy"]
{len(tray)} things on the tray.
-> DONE
```

No annotation fixes a `Conflicted` collection — declaring the tray
`array<int>` doesn't make `"brandy"` a number. Either the elements agree,
or they were never one collection.

### The empty-literal rule

`#[]` and `#{}` are the interesting case, and worth being precise about,
because every inventory, queue, and memo table starts empty. An empty
literal carries **no evidence** — no element to read a type from — so its
type must arrive from context: an ascribed binding, or an already-typed
slot it's being assigned into. If nothing constrains it, inference runs
out of evidence and strict mode reports the escape as `E065` —
"`stock`'s temp `crates` escapes strict inference as Unknown — annotate
or restructure":

```ink,error(E065)
-> stock

=== stock ===
~ temp crates = #[]
{len(crates)} crates in the yard.
-> DONE
```

Note what is *not* evidence: `len(crates)` accepts every collection, and
even a later `push` narrows the element only once the array's type is
known — use is not ascription. The fix is the one the message names.
Ascribe the binding, and the literal takes its type from the declaration:

```ink
~ temp cellar: array<string> = #[]
~ push(cellar, "amber ale")
~ push(cellar, "black cider")
The cellar ledger holds {len(cellar)} casks: {cellar}.
-> END
```

```text
The cellar ledger holds 2 casks: [amber ale, black cider].
```

This is one instance of a language-wide posture rather than a special
collection rule: a value born without a type must be told one at birth.
The same rule types a bare `none` (`E107` — see below), and under
`types = gradual` the same empty literal simply stays `Unknown` and
defers to runtime behavior, unchecked.

## An index is a claim

Reading and writing elements uses postfix indexing, in expression
position, chaining as deep as the data goes:

```ink
VAR data = 0
VAR result = 0

~ {
    data = #[#{"a": #[1, 2, 3], "b": #[4, 5, 6]}, #{"a": #[7, 8, 9], "b": #[10, 11, 12]}]
    data[0]["a"][2] = 30
    result = data[0]["a"][2] + data[1]["b"][0] + data[0]["b"][1]
}

Result is {result}.
-> END
```

```text
Result is 45.
```

The contracts behind that syntax are few, and each is a doctrine line you
can carry around:

**`a[i]` is a claim.** Writing an index asserts "a valid element lives at
`i`." Out of bounds — reading *or* writing — is a turn-terminating
runtime fault, not a `null`, not silent growth. An index you computed
wrong is a bug, and the fault surfaces it where it happened.

**A write never grows the array.** `a[len(a)] = v` doesn't append; it
faults. Growth has its own verbs — `push` and `insert` — which say so in
their names.

**`m[k]` read is a claim; `m[k] = v` write is not.** Reading a missing
key faults — "I expect it there" is the faulting read's contract. Writing
a missing key **inserts** the pair; writing an existing key overwrites in
place:

```ink
VAR memo = 0

~ {
    memo = #{}
    memo["a"] = 1
    memo["a"] = 2
    memo["b"] = 3
}

fresh_a={memo["a"]}, fresh_b={memo["b"]}, size={len(memo)}
-> END
```

```text
fresh_a=2, fresh_b=3, size=2
```

The full fault roster for indexing:

| Situation | What happens |
|---|---|
| `a[i]` / `a[i] = v` with `i` outside `[0, len(a))` | Fault — index out of bounds |
| `m[k]` read with `k` not present | Fault — no such key |
| `m[k] = v` with `k` not present | Inserts the pair |
| Indexing a value that isn't indexable | Fault — not indexable |
| An array index that isn't an `int` | Fault — invalid index |
| A map key outside the key domain | Fault — invalid key type |

Faults are deterministic and total in the value-model sense — recorded in
the transcript, reproduced identically on replay, with no in-story
`try`/`catch` (v1 scripts are infallible from the inside; what a host
does about a fault is host policy). And because collections are values,
every indexed write lowers to a read-modify-write on the root variable —
observable behavior is always "as if" a fresh copy, with the runtime
sharing storage until an owner actually writes. The mechanics of that
lowering, chained writes included, live in
[Indexing & Mutation](indexing.md).

## Maps: the key domain and insertion order

Map keys are scalars — `int`, `string`, or `bool` — the same ratified key
domain every map-keyed operation in the value model uses. A key literal
that is statically outside the domain (a float, an array, a map) gets a
compile-time **warning**, `E106` ("map-literal key is outside the
int/string/bool key domain"); a dynamic key expression that turns out bad
at runtime is the corresponding turn-terminating construction fault. One
domain, checked early where visible, enforced at runtime always.

Order is worth trusting, because it's guaranteed:

- **Iteration order is insertion order.** `for k in m` visits keys in the
  order they were inserted; `keys(m)` and `values(m)` reify the same
  order as eager array snapshots.
- **Overwriting keeps position; removing shifts survivors down.** Writing
  an existing key never moves it; a re-inserted key goes to the end only
  if it was actually removed first.
- **Equality ignores order.** `#{"a": 1, "b": 2} == #{"b": 2, "a": 1}` is
  true — equality compares content (key → value pairs), never
  construction history. Only equality ignores order; iteration and
  serialization keep it.

```ink
VAR m = 0

~ {
    m = #{"z": 1, "a": 2, "m": 3}
}

Keys is {keys(m)}. Values is {values(m)}.
-> END
```

```text
Keys is [z, a, m]. Values is [1, 2, 3].
```

Determinism here is a language guarantee, not an implementation accident:
a story that iterates a map prints the same lines on every run and every
platform, and a seeded replay reproduces them byte-for-byte.

## The verb surface

Beyond literals and indexing, collections are worked through a small
family of free-function verbs. Two conventions organize the whole
surface, and they're worth learning as *rules* because every future verb
obeys them:

**Imperative verbs mutate in place; past-participle verbs return a new
collection.** `sort(a)` sorts `a`; `sorted(a)` hands back a sorted copy
and leaves `a` alone. The verb carries the mutation signal, and the
confusion lattice is closed from both sides: an in-place verb returns
nothing (using one in expression position is `E056`), and a functional
verb doesn't touch its argument.

**Mutating verbs demand a place, not a value.** The first argument of
`push`/`insert`/`remove`/`remove_at`/`clear`/`sort`/`sort_by` must be an **lvalue** —
a variable, temp, or indexed path — because the mutated collection has to
be written back somewhere. Handing one a temporary is `E055` — "`push`
mutates its first argument — bind it to a variable first":

```ink,error(E055)
~ push(#["ale"], "cider")
-> DONE
```

Mutating a temporary would mutate nothing; the error refuses the lost
write. A wrong argument count on these verbs is its own targeted error
(`E058`), naming the expected signature.

The reading verbs, at a glance — signatures in the standard display
notation (*display notation; `T` is not writable in source*):

| Verb | Signature | Notes |
|---|---|---|
| `len` | `len(x: [T] \| [K: V] \| string \| range): int` | element / entry / character / span count |
| `contains` | `contains(a: [T], x: T): bool` | element scan (arrays); **key** test on maps |
| `contains_value` | `contains_value(m: [K: V], v: V): bool` | content-equality scan over values; O(n) and honest about it |
| `keys` / `values` | `keys(m: [K: V]): [K]` | eager snapshots, insertion order |
| `index_of` | `index_of(a: [T], x: T): Option[int]` | first match, or `none` |
| `first` / `last` | `first(a: [T]): Option[T]` | `none` on empty |
| `min` / `max` | `min(a: [T]): Option[T]` | doctrine order; `none` on empty |
| `get` | `get(m: [K: V], k: K): Option[V]` | the non-faulting map read |
| `sorted` / `sorted_by` | `sorted(a: [T]): [T]` | functional twins of `sort`/`sort_by` |

And the mutators — statement-only, lvalue-first:

| Verb | Signature | Notes |
|---|---|---|
| `push` | `push(a: [T], x: T)` | append |
| `insert` | `insert(a: [T], i: int, x: T)` | insert at `i`, `0 ≤ i ≤ len` — the one array write allowed to reach the end |
| `insert` (map) | `insert(m: [K: V], k: K, v: V)` | today's spelling; `m[k] = v` is the ruled one — see below |
| `remove_at` | `remove_at(a: [T], i: int)` | remove at `i`; out of bounds faults |
| `remove` | `remove(m: [K: V], k: K)` | total — removing an absent key is a no-op |
| `clear` | `clear(m: [K: V])` | empty in place |
| `sort` / `sort_by` | `sort(a: [T])` | in-place ordering — next section |
| `pop` | `pop(a: [T]): Option[T]` | the hybrid: removes *and* returns the last element — the one mutator legal in expression position |

Two postures hiding in that table deserve their doctrine lines:

**A deletion is a wish; an index is a claim.** `remove(m, k)` on an
absent key is a no-op — you wished the key gone, and gone it is,
idempotently. `remove_at(a, i)` out of bounds faults — you claimed an
element existed at `i`, and it didn't. Both postures are correct for
their domain, but they used to share one verb name — `remove` covered
both, which was an accident, not a decision (issue #1484 caught it:
nothing about a map-key removal implies an array-index removal, or vice
versa). The fix is naming, not flattening: `remove_at` joins the `_at`
faulting-index family with `char_at`, leaving `remove` to mean exactly
one thing — identity-based, idempotent-total removal (map keys today;
flags values once flags land). There is no compatibility shim: a
pre-#1484 `remove(array, i)` call site is `E149` (issue #1532) under
`types = strict`, the brink dialect's own implicit default; under
`types = gradual` it stays a runtime `NotIndexable` fault.

**One spelling per concept, eventually.** Today's dialect ships
`insert(m, k, v)` as a slice-1 free function, but the ruled native
surface reserves the map-`insert` verb: `m[k] = v` *is* insertion, and
one concept gets one spelling. Prefer the indexed write; the free-call
form is a compat spelling with an expiry date. (Array `insert` — a
genuinely distinct concept, positional insertion — stays.)

Effects, briefly, since every verb carries an inferred row (see
[Effects](effects.md)): the mutators write their receiver's root
variable, the readers read; none of them emits content; and any verb that
can fault (wrong container type, out-of-bounds index, unorderable
elements) carries that fault in its row. You never annotate any of this —
the compiler harvests it from the call.

## When the world doesn't have one

Half the verbs above return `Option[T]`, and the reason is the language's
absence doctrine in one line: **a fault says "your program is wrong";
`Option` says "the world didn't have one."** An out-of-bounds index is
the first kind — a bug, surfaced loudly. An empty array's `max`, a search
that found nothing, a key that was never filed — those are the second
kind: honest answers to reasonable questions, and they come back as a
value you can test.

An `Option[T]` is either `some(x)` — the world had one, here it is — or
`none`. `some(x)` always renders as `some(x)`; a bare `none` at the
**final** value of an interpolation renders as nothing at all — absence
rendering as absence ([Option and Absence](option.md#how-option-prints)
has the full display rule). You test it with explicit equality:

```ink
~ temp tab = #[4, 7, 2, 5]
~ temp rooms = #{"Mira": 3, "Old Tom": 1}
Heaviest night on the tab: {max(tab)}.
Edda's room: {get(rooms, "Edda")}. Mira's room: {get(rooms, "Mira")}.
~ temp settled = pop(tab)
Settled the last entry, {settled} — {len(tab)} remain.
{index_of(tab, 7) == some(1): The seven-coin night is still second in the ledger.}
{get(rooms, "Edda") == none: No Edda on the register tonight.}
-> END
```

```text
Heaviest night on the tab: some(7).
Edda's room: . Mira's room: some(3).
Settled the last entry, some(5) — 3 remain.
The seven-coin night is still second in the ledger.
No Edda on the register tonight.
```

(Edda's room prints nothing, not the word `none` — that's the
interpolation boundary at work, not a rendering bug.)

Two fences keep the doctrine honest. First, `Option[T]` has **no
truthiness** — `{first(tab): …}` is not "is there a first element", it is
a compile error under strict (`E116`: "an `Option[T]` has no truthiness —
test `== none` / `== some(x)` explicitly") and a runtime fault under
gradual:

```ink,error(E116)
-> ledger

=== ledger ===
~ temp tab = #[4, 7, 2]
{first(tab): Somebody still owes.}
-> DONE
```

A truthiness test is a quiet coercion of exactly the kind
`Option[T] ≠ T` exists to ban — it blurs "the world had one" into "the
value was truthy." Second, a bare `none` carries no element type, so a
fresh un-annotated `VAR gap = none` is `E107` ("bare `none` needs a type
from context") — the empty-literal rule again, wearing its Option hat.

`Option[T]` appears in this chapter's signatures and diagnostics but is
display notation — you cannot write it in an annotation today. The full
doctrine — `x or default` coalescing, the display-boundary forgiveness,
`filter_map` — belongs to the Option chapter of the book's
reorganization; this chapter uses only what you've just seen:
constructors `some(x)`/`none`, and explicit `==` tests.

## Sorting the ledger

The ordering family is four verbs in the two-convention grid: `sort(a)` /
`sorted(a)` order by the language's **doctrine order**; `sort_by(a, cmp)`
/ `sorted_by(a, cmp)` order by a comparator you supply.

```ink
~ temp tab = #[3, 1, 2, 1]
~ temp fair = sorted(tab)
For the magistrate: {fair}; the ledger itself still reads {tab}.
~ sort(tab)
Now the ledger agrees: {tab}.
~ temp words = #["pear", "apple", "fig"]
Alphabetical stock: {sorted(words)}.
-> END
```

```text
For the magistrate: [1, 1, 2, 3]; the ledger itself still reads [3, 1, 2, 1].
Now the ledger agrees: [1, 1, 2, 3].
Alphabetical stock: [apple, fig, pear].
```

**What orders**: ints and floats together (the numeric join); bools
(`false < true`); strings, lexicographic by Unicode scalar value (locale
collation is the intl pipeline's business, like casing); and arrays,
lexicographic element-wise, recursively. **What doesn't**: maps, divert
targets, and anything else without a defined order — sorting those is a
fault, not a shrug. Structs and enums order only via an explicit
`compare` protocol impl — field declaration order is never silently
promoted to semantics.

**Sorting is stable.** Equal elements keep their input order — sort the
tab by amount and two 4-coin nights stay in chronological order. Stable,
deterministic, replay-identical: sorting is part of the determinism
contract, not an exception to it.

**Sort never implies dedup.** Ordering and equality are separate
questions; a sort with ties drops nothing.

### Comparators are a contract

A comparator is an ordinary function of two elements returning an `int` —
negative for "a first", zero for "tie", positive for "b first":

```ink
~ temp owed = #[3, 9, 5]
~ sort_by(owed, #fn(largest_first))
Calling in debts from the top: {owed}.
-> END

=== function largest_first(a: int, b: int): int ===
~ return b - a
```

```text
Calling in debts from the top: [9, 5, 3].
```

The contract: a comparator must be **pure and silent** — no writes, no
output, no drawing randomness, and no reading story state either, because
the order must depend only on the two comparands — and it must describe a
consistent total order. A comparator
the compiler can prove breaks that contract is `E119` — "`sort_by`'s
comparator `counting` reads tally; writes tally — a comparator must be a
pure, silent `fn(T, T): int` (stdlib-spec §4b: the order must depend only
on the two comparands)":

```ink,error(E119)
VAR tally = 0

~ temp order = #[3, 1, 2]
~ sort_by(order, #fn(counting))
{order}
-> END

=== function counting(a: int, b: int): int ===
~ tally = tally + 1
~ return a - b
```

The check is exceedance-only — it fires on what it can prove, never on
what it can't. An *inconsistent* comparator (one that contradicts itself)
can't always be caught statically; the runtime may fault on detected
inconsistency, and the guarantee floor is "the result is some permutation
of the input, never worse." A comparator that faults mid-sort faults the
turn, like any other fault.

### NaN, dev, and prod

Floats bring one genuinely awkward guest to the sorting table: `NaN`,
which IEEE comparison refuses to order. The language's answer is a
doctrine with a mode knob, and it's worth knowing as an author even
though you'll mostly meet it in a debug session:

- **Arithmetic is NaN-total.** `sqrt(-1.0)` is `NaN`, flows through `+`
  and `*` freely, and never faults. Ordering contexts are where it stops.
- **Dev mode faults, loudly.** A `NaN` operand reaching
  `sort`/`sorted`/`min`/`max` is a turn-terminating fault — the upstream
  arithmetic bug surfaces at its first ordering consumption, where you
  can still find it. Dev is the default.
- **Prod mode keeps going.** The same `NaN` is *placed* by a pinned total
  order — `NaN` sorts greater than everything, `NaN` ties with `NaN`,
  `-0` ties with `+0` — and execution continues. Placement is
  deterministic and save/replay-safe; no data is fabricated, every
  element survives.

On NaN-free data the two modes agree exactly, and both cohere with `<`
and `==` — turning prod on never changes a clean story's output. The
dev/prod split is *fenced* to exactly this kind of case: it exists only
where the prod behavior is total and fabricates nothing. Placement
qualifies; fabrication never does — `int("potato")` and out-of-bounds
indexing fault in every mode, forever. The knob's home is project
configuration with a host-API override (`Story::set_exec_mode` /
`FlowInstance::set_exec_mode` — see [Runtime API](../reference/runtime-api.md));
the mode is a host/build knob, never story state, never saved.

One asymmetry worth knowing: `sort_by` is **not** in the dev NaN-fault
list. Your comparator owns the element semantics — `NaN` never reaches
the ordering machinery as a comparison result, so `sort_by` faults only
on its own terms (comparator dispatch, a non-`int` return, whatever the
comparator's body faults on, detected inconsistency).

The deeper doctrine — comparison operators staying frozen IEEE, the
`compare` protocol, heaps — belongs to the Ordering chapter of the book's
reorganization; this is the working author's share of it.

## Ranges

A range is a span of integers as a value: `0..10` is 0 through 9
(half-open), `1..=6` is 1 through 6 (inclusive). Ranges are real values —
they store, print, compare, save, and restore like any other — and they
behave as the read-only array of their integers:

```ink
~ temp die = 1..=6
~ temp span = 0..10
The dice corner chalks its spans: {string(die)} and {string(span)}.
Six faces: {len(die)}; first step {span[0]}, last step {span[9]}.
Same span, either spelling: {die == 1..7}.
~ temp calls = ""
~ {
    for n in 2..=4 {
        calls = calls + " " + string(n)
    }
}
The crier calls{calls}.
{0..0: An empty span fires.|An empty span never fires.}
-> END
```

```text
The dice corner chalks its spans: 1..=6 and 0..10.
Six faces: 6; first step 0, last step 9.
Same span, either spelling: true.
The crier calls 2 3 4.
An empty span never fires.
```

The contracts, each an echo of one you've already met:

- **Indexing is the same claim.** `span[i]` is `start + i`, and out of
  bounds faults exactly like an array.
- **Equality is content.** `1..=6 == 1..7` is true — a range *is* the
  integer sequence it denotes, and all empty ranges are equal to each
  other. Display and the wire keep the written form (`1..=6` prints as
  you spelled it); equality doesn't care.
- **Emptiness is legal and load-bearing.** `0..n` with `n = 0` iterates
  zero times and is false in condition position — that's what makes
  `for i in 0..len(a)` safe on an empty array, no guard needed.

Where emptiness is *not* acceptable, the language says so in the type:
drawing a die roll with `int(1..=6)` requires an **inhabited** range, the
language's first value refinement — a statically-empty literal is a
compile error, and computed bounds pass through the `non_empty(r)`
validator, which returns `Option` (`E117` is strict mode's enforcement;
gradual faults at runtime). That story — parse-don't-validate, and every
draw being an effect — is the Randomness chapter's; iteration in full
(`for`, and the pure verb trio when it lands) is the Iteration chapter's.

> **Views — a performance contract, ruled ahead of its verbs.** Slicing
> verbs (`slice`, `split`, `trim`) haven't landed in the dialect yet, but
> their semantics are already ruled: when they arrive they return
> **views** — O(1), non-allocating windows onto the original storage.
> A view is a representation, not a type: sharing is unobservable (values
> are values; every observation behaves as a copy), saves and the wire
> always materialize, and the O(1) promise is a regression-guarded
> contract, not an optimization that may quietly vanish. Nothing to do
> today — this sidebar exists so "slice is cheap" is a fact you can plan
> around, not folklore.

## Reference: the diagnostics in this chapter

| Code | Fires when | Policy |
|---|---|---|
| `E051` | collection syntax in a `strict-ink` project | dialect gate |
| `E055` | a mutator's first argument is not an lvalue | both |
| `E056` | a statement-only mutator used in expression position | both |
| `E058` | mutator argument count mismatch | both |
| `E065` | an unconstrained empty literal escapes inference as `Unknown` | strict |
| `E066` | collection elements can't unify — the type is `Conflicted` | strict |
| `E076` | a map key in a declaration default doesn't fold to a scalar | both |
| `E077` | a non-constant element/value inside a declaration-default literal | both |
| `E083` | a declaration default that isn't compile-time constant | both |
| `E106` | a statically-visible map-literal key outside `int`/`string`/`bool` | both (warning) |
| `E107` | a bare `none` with no type from context | both |
| `E116` | an `Option[T]` used as a condition — no truthiness | strict (runtime fault under gradual) |
| `E117` | `int(r)` over a range not proven inhabited | strict (runtime fault under gradual) |
| `E119` | a `sort_by`/`sorted_by` comparator provably exceeds pure·silent | both |

## Where this is ruled

- **Collection surface: literals, indexing, stdlib slice 1** —
  `docs/t1b-surface-spec.md` §§3–5; value semantics and fault posture,
  `docs/value-model-spec.md` (ratified).
- **The indexing contract** (`m[k]` read-faults / write-inserts) —
  decision log 2026-07-15 (#856).
- **The Option package and the absence flips** (`find`/`index_of`/
  `get`/`first`/`last`/`min`/`max`/`pop` → `Option`) —
  `docs/stdlib-spec.md` §§1.1/1.4, §§4–5; decision log 2026-07-18
  ("Option[T] pulled forward"); F27 no-truthiness ruled 2026-07-19.
- **Mutation posture and the naming law** (imperative in-place /
  past-participle functional, lvalue receivers) — `docs/stdlib-spec.md`
  §4; decision log 2026-07-18 ("Mutation posture").
- **Maps** (key domain, insertion order, `contains_value`, `insert`
  reserved, remove-total) — `docs/stdlib-spec.md` §5; decision log
  2026-07-18 ("Maps ruled"); order-insensitive equality, decision log
  2026-07-18 ("Map/record equality is insertion-order-insensitive").
- **The `remove`/`remove_at` split** (seq remove-by-index renamed
  `remove_at`, `remove` narrowed to identity-based idempotent-total
  removal) — issue #1484; decision log 2026-07-26 ("Quick-docket
  closures").
- **The ordering doctrine and the sort family** (doctrine order, stable
  sort, dev/prod NaN posture, comparator contract) —
  `docs/stdlib-spec.md` §4b; decision log 2026-07-18 ("The ordering
  doctrine"); F0 (`sort_by` in-place) and the knob's config home ruled
  2026-07-19; F14/F29 as-built amendments, NS-A4 (#1110).
- **Ranges as real values; content equality; the inhabited-range
  refinement** — `docs/stdlib-spec.md` §7; F7/F8 ruled 2026-07-19; F30
  content equality ratified 2026-07-19 (delegated batch); NS-A5 (#1136).
- **Views** — `docs/stdlib-spec.md` §3b.
