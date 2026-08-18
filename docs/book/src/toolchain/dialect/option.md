# Option and Absence

The Last Light's register knows who signed in tonight — and, just as
usefully, who didn't:

```ink
~ temp rooms = #{"Mira": 3, "Old Tom": 1}
~ temp mira = get(rooms, "Mira")
~ temp edda = get(rooms, "Edda")
The innkeeper runs a finger down the register.
Mira: {mira}. Edda: {string(edda)}.
{mira == some(3): Mira is upstairs in room 3, same as always.}
{edda == none: No Edda tonight — the road must have kept her.}
-> END
```

```text
The innkeeper runs a finger down the register.
Mira: some(3). Edda: none.
Mira is upstairs in room 3, same as always.
No Edda tonight — the road must have kept her.
```

Nothing went wrong in that scene. Asking the register about Edda was a
perfectly reasonable question, and *the register answered it*: `none`. The
answer is a real value — it printed, it compared, it could have been stored
or passed along — and the story kept running. That value's type is
`Option<T>`, this chapter's subject, and the doctrine it carries is one line
long: **a fault says "your program is wrong"; Option says "the world didn't
have one."**

(That scene prints Edda's line through `string(edda)` rather than a bare
`{edda}` — a deliberate choice, not an oversight. A bare `{edda}` would
print nothing at all: [How Option prints](#how-option-prints), later in
this chapter, is where that rule belongs.)

> **Current spelling** — examples in this chapter compile in today's brink
> dialect: collection literals carry the `#[…]`/`#{…}` sigils and the Option
> verbs are free calls (`get(rooms, "Edda")`, `find(s, sub)`). The ruled
> native `.brink` spelling for method-position calls (`rooms.get("Edda")`)
> and the `as`-binding (B1b, issue #1475 — see the callout later in this
> chapter) have both landed on the native surface; this chapter's own
> respell to that surface is separate, later work. The `x or default`
> coalescing form has landed (B1, issue #1460) — but **only on the native
> `.brink` surface**: `or` in the *brink dialect* this chapter's examples
> use (the `~`-prefixed, `#[…]`-sigil syntax above) is still ink's boolean
> or (an alias for `||`), unchanged and oracle-frozen; a native-surface
> `.brink` file can already write `get(rooms, "Edda") or "no one"`.

## Absence is a value, not a fault

The [Collections](literals.md) chapter drew the line this chapter stands on:
an out-of-bounds index **faults**, because an index is a claim — you
asserted an element existed and you were wrong, and that's a bug the runtime
surfaces where it happened. But an empty array's `max`, a search that found
nothing, a guest who never signed the register — those aren't bugs. They're
honest answers to reasonable questions, and a language that can't say them
politely forces authors into one of two old traps:

- **The sentinel.** Return `-1` for "not found", `0` for "empty", and hope
  nobody ever does arithmetic on the flag value. The bug this breeds is
  silent and famous: the sentinel *is* a valid-looking number, so absence
  quietly becomes data.
- **The fault.** Treat "the world didn't have one" like "your program is
  wrong" and end the turn. Now every lookup needs a defensive guard, and
  expected absence — the bread and butter of narrative state — is
  indistinguishable from a genuine bug.

Brink shipped neither. `find` and `index_of` were headed for `-1` sentinels
and the empty-array extremums for faults when the Option ruling caught them
— the sentinels died unshipped, and every absence-shaped verb was flipped to
return `Option<T>` before any story could depend on the bad answers.

An `Option<T>` is one of exactly two things:

- **`some(x)`** — the world had one, and here it is.
- **`none`** — the world didn't have one. Not zero, not `-1`, not a fault:
  a first-class value that says *absence*, and says nothing else.

The doctrine cuts both ways, and the faults are still there for the bugs.
Asking a **malformed question** — `find` on a number, `min` over a map,
`get` with a key outside the key domain — is a turn-terminating fault, never
a `none`. So is an out-of-bounds index, in every mode, forever. The line to
carry around: *absence never faults; malformed questions always do.*

| Situation | Answer |
|---|---|
| `max` of an empty array | `none` — absence |
| `get` of a key never filed | `none` — absence |
| `find` of a substring that isn't there | `none` — absence |
| `a[i]` out of bounds | fault — an index is a claim |
| `min` over a map (wrong container) | fault — malformed question |
| `min` over unorderable elements | fault — malformed question |

## The verbs that answer with Option

Ten verbs across the stdlib return `Option<T>` today — every place the
language asks the world a question the world might honestly have no answer
to. Signatures in the standard display notation (*display notation; `T` is
not writable in source*):

| Verb | Signature | `none` means |
|---|---|---|
| `find` | `find(s: string, sub: string): Option<int>` | substring absent (USV index when present) |
| `index_of` | `index_of(a: [T], x: T): Option<int>` | no element equal to `x` |
| `first` | `first(a: [T]): Option<T>` | array empty |
| `last` | `last(a: [T]): Option<T>` | array empty |
| `min` | `min(a: [T]): Option<T>` | array empty |
| `max` | `max(a: [T]): Option<T>` | array empty |
| `pop` | `pop(a: [T]): Option<T>` | array empty — nothing removed |
| `get` | `get(m: [K: V], k: K): Option<V>` | key absent |
| `pick` | `pick(x: [T] \| range): Option<T>` | nothing to draw from |
| `non_empty` | `non_empty(r: range): Option<NonEmptyRange>` | the range was empty |

Three of these deserve a word beyond the table. `pop` is the hybrid the
Collections chapter flagged — it removes the last element *and* hands it
back, so an empty array means "nothing removed, and here's the `none` to
prove it." `pick` draws a random element, so its `none` (an empty array or
range has nothing to draw) travels with the whole randomness story — seeds,
draws-as-writes — in the Randomness chapter. And `non_empty` is the
validator for the inhabited-range refinement: its `some` payload is the
proven-inhabited range that `int(r)` demands. Also that chapter's story.

Every verb here carries its inferred effect row like any other
([Effects](effects.md)): the readers read, `pop` writes its receiver, and
the fault possibility for malformed questions rides in the row. You
annotate none of it.

## Constructing and comparing

You'll mostly *receive* Options from the verbs, but both shapes are
writable directly: `some(expr)` wraps any value, and `none` is the absence
literal. Equality is structural and total over both — `none == none`;
`some(x) == some(y)` exactly when `x == y` — and `==`/`!=` are the *only*
operators defined on Options. There is no `<` between them (Options don't
order — sorting an array of them is a malformed question), and no
arithmetic, which is the next section's subject.

```ink
~ temp stock = "ale, cider, bread"
~ temp ale = find(stock, "ale")
~ temp gin = find(stock, "gin")
ale: {ale}, gin: {gin}
{ale == some(0): The ale is first on the slate.}
{ale != gin: One of these is on the shelf and one is not.}
~ gin = some(99)
{gin == some(99): The gin arrived on the evening cart.}
-> END
```

```text
ale: some(0), gin:
The ale is first on the slate.
One of these is on the shelf and one is not.
The gin arrived on the evening cart.
```

(`gin`'s line ends right after the colon — a bare `{gin}` is a `none` at
the interpolation boundary, and renders as nothing;
[How Option prints](#how-option-prints) explains why.)

Note the reassignment: an Option variable moves freely between `some(…)`
and `none` over its life — "what the register currently says about the gin"
is exactly the kind of state Options are for. Options nest, too:
`some(none)` is a real value and it is *not* equal to `none` — a box with
an empty box inside is not an empty shelf. You'll rarely want that, but
equality won't blur it for you.

One birth rule, which you already met wearing its empty-collection hat in
[Collections](literals.md): **a value born without a type must be told one
at birth.** A bare `none` carries no element type, so a fresh un-annotated
declaration initialized from one has nothing to be — that's `E107`, in both
dialects and under both type policies:

```ink,error(E107)
VAR reservation = none

The book lies open on the counter.
-> END
```

The message names the fix: "`reservation` is declared from a bare `none`,
which carries no element type — initialize from `some(x)` or an
Option-returning verb (`find`/`get`/`pop`/…) instead." Every *other* `none`
position — an assignment to an existing Option slot, a comparison operand,
a call argument — has context by construction and is fine.

## `Option<T>` is not `T`

The type `Option<int>` and the type `int` are different types, and the
checker holds that line everywhere: no implicit unwrap, no coercion, no
"it's probably `some`, treat it as the number." The strictness is the whole
point — if `Option<T>` quietly became `T` wherever convenient, the sentinel
bug would be back with better manners, and absence would go silent again
exactly where it matters.

Under strict types (the brink dialect's default), mixing the two is the
familiar `Conflicted` diagnosis from [Values & Types](types.md) — the slot
is being used as two irreconcilable types. This fails with `E066`,
"`ledger`'s temp `floor` is Conflicted under strict types — its uses
disagree on its type":

```ink,error(E066)
-> ledger

=== ledger ===
~ temp rooms = #{"Mira": 3, "Old Tom": 1}
~ temp floor = get(rooms, "Mira")
~ floor = 2
Mira sleeps on floor {floor}.
-> DONE
```

`get` answered `Option<int>`; the assignment insists on `int`; no
annotation reconciles them. The same collision caught anywhere else —
an Option operand in arithmetic, an Option passed where a plain value is
declared — is the same conflict. Under `types = gradual` the checker lets
the unknown ride and the *runtime* holds the line instead: `some(1) == 1`
is a turn-terminating type fault, not `true` and not a coercion. Either
way, no policy blurs `some(1)` into `1`.

Options flow through your own functions like any other value, and
inference handles the signatures exactly as [Values & Types](types.md)
described — from the body, bottom-up:

```ink
VAR rooms = #{"Mira": 3, "Old Tom": 1}

Mira: {room_of("Mira")}. Edda: {string(room_of("Edda"))}.
-> END

=== function room_of(name: string) ===
~ return get(rooms, name)
```

```text
Mira: some(3). Edda: none.
```

`room_of` settles as `(string) -> Option<int>` with no annotation — the
body's `get` is all the evidence needed.

`Option<T>` is both **inferable and annotatable** (issue #1552): it
appears in inferred signatures, in diagnostics, and throughout this
chapter's tables, and you can also write it yourself — `~ temp best:
Option<int> = none` resolves exactly like `int` or `Array<int>` would.
`Weighted<T>` gained the same annotation spelling in the same change.
`range` is the one remaining construction-only builtin: no annotation
form yet, pending demonstrated demand.

## No truthiness

The oldest ink idiom — a bare value in condition position, nonzero meaning
true — does **not** extend to Option, on purpose. `{get(rooms, "Edda"): …}`
reads like "is Edda registered", but a truthiness test is a quiet coercion
of exactly the kind `Option<T> ≠ T` exists to ban: it blurs "the world had
one" into "the value was truthy," and it reintroduces the silent-absence
bug class one condition at a time. So Option has *no* truthiness, anywhere,
and the condition-position error tells you the honest spelling. This fails
with `E116`, "an `Option[T]` has no truthiness — test `== none` /
`== some(x)` in the condition (F27, docs/stdlib-spec.md §1.6)":

```ink,error(E116)
-> register

=== register ===
~ temp rooms = #{"Mira": 3, "Old Tom": 1}
{get(rooms, "Edda"): Somebody named Edda is upstairs.}
-> DONE
```

The rule covers every condition position — `if`/`while` conditions,
`{cond: …}` conditional branches and their inline forms, choice guards,
await conditions — and negation doesn't launder it: `{!opt: …}` is the same
error. Under strict types it's the compile error above, for every condition
the checker can statically classify; a condition it can't see through stays
silently unchecked at compile time. Under gradual there is no compile-time
check at all. Both residues meet the same runtime backstop: an Option
reaching a truthiness evaluation is a turn-terminating fault ("an Option
has no truthiness — test `== none` / `== some(x)` explicitly"), never a
silent false.

The contrast with the idiom that *does* survive strict is worth holding
side by side. A visit count in condition position (`{market: …}`) is a
plain `int` with a deliberately preserved, scoped truthiness — see
[Values & Types](types.md). An Option in condition position is an error in
every mode. The difference isn't taste: the visit count *is* a number being
tested as a number, while an Option in a condition is a category mistake
the explicit forms fix for the cost of one comparison.

## Getting the value out

Here is the honest state of the surface: today you can *make* Options,
*print* them, and *compare* them — and comparing is the only door from
`Option<T>` back to `T`-shaped decisions. There is no unwrap verb, no
default-extraction verb, no per-verb `get_or` family. That narrowness is
deliberate (the ruling that shipped Option explicitly folded `get_or` into
the coalescing form to come — one spelling per concept, and no stopgap
verbs that would outlive their excuse), but it is real, and until the
planned ergonomics land (the callout at the end of this section) you
should know the three working idioms.

**Compare against the candidates.** When the interesting values are few,
explicit equality is the whole job — `{ale == some(0): …}`,
`{edda == none: …}` — as every example so far has done.

**Ask, then claim.** The Option verbs have faulting siblings that return
plain `T` — `get(m, k)` pairs with the claiming read `m[k]`, `first(a)`
with `a[0]`. Establish presence with the Option (or with `contains`), then
make the claim, which is now justified:

```ink
~ temp rooms = #{"Mira": 3, "Old Tom": 1}
{get(rooms, "Mira") != none:
    Mira's key hangs on hook {rooms["Mira"]}.
- else:
    No key to fetch tonight.
}
-> END
```

```text
Mira's key hangs on hook 3.
```

This is the two contracts from [Collections](literals.md) composed, not a
trick: the question that tolerates absence gets the Option; the read that
asserts presence gets the fault-backed claim; the guard is what turns the
first into a license for the second.

**Fold your own.** Some verbs have no claiming sibling — there is no total
`min` to graduate to once you've checked the array is non-empty. When you
need the *value* of an extremum rather than a comparison against it, write
the loop; it's four lines, and it says exactly what it computes:

```ink
~ temp tab = #[4, 7, 2, 5]
~ temp heaviest = 0
~ {
    for coins in tab {
        if coins > heaviest {
            heaviest = coins
        }
    }
}
The heaviest night on the tab: {heaviest} coins — the ledger says {max(tab)}.
-> END
```

```text
The heaviest night on the tab: 7 coins — the ledger says some(7).
```

> **Landed on the native surface, still planned for this (brink-dialect)
> chapter (B1/B1b, issues #1460 and #1475).** The ruled Option package
> includes its ergonomics, and both halves now compile on the native
> `.brink` surface. The coalescing form `x or default` collapses an Option
> into a value (`get(rooms, "Edda") or 0`), chaining left-to-right and
> staying optional until the final non-Option fallback
> (`get(m, k) or get(m, k2) or 0`). It **short-circuits** (ruled, issue
> #1471): a fallback is evaluated only when everything to its left came
> back `none`, so `x or expensive()` never pays for `expensive()` when `x`
> is already `some(_)`. The **`as`-binding** tests and unwraps
> in one move, and it is one construct in both of the language's condition
> positions — the statement form and the template form, riding the ruled
> `{if …}` spelling:
>
> ```brink
> if get(rooms, "Edda") as r {
>     // `r` is a plain `int` here — the Option is already unwrapped
> }
> {if get(rooms, "Edda") as r: room {r} it is else: no room tonight}
> ```
>
> The binding is immutable, typed `T` from the condition's `Option<T>`,
> scoped strictly to the success arm (an `else` never sees it), and
> rebinds every iteration in `while`. For v1 the binding must be the
> **entire** condition — composing it with `&&`/`||` is an error (`E145`);
> let-chains can land later, additively. An `as` in a *choice guard* is
> ruled (capture-at-presentation, by value) and now implemented too — the
> guard's binding captures into the same frame slot the pending choice's
> thread-fork snapshot already carries across selection, so the picked
> body sees the value the player saw (`E146`, "not yet supported", is
> retired). Nothing in *this* chapter changes either way — these
> examples are brink-dialect, where `or` stays ink's boolean or and there
> is no `as` binding at all; the chapter's own respell to the native
> surface is separate, later work.

## How Option prints

`some(v)` is total and boring forever, by ruling (F28): it renders
`some(<v>)` everywhere it appears — in interpolation, and identically
through `string(x)`, which keeps its everything-in, never-fails contract.
`none` is where the two consumers **diverge**, by a second, later ruling
(§1.6b, Track B4): `string(none)` still renders the total, boring `"none"`
forever — F28's totality, preserved for that one intrinsic — but a `none`
that is the **final** value of an interpolation renders as *nothing at
all*. Absence rendering as absence is the honest narrative meaning; a
`none` that leaked into your prose as the word "none" was always a
debugging artifact, not something a player should read.

```ink
~ temp tab = #[4, 7, 2]
~ temp empty: Array<int> = #[]
Tonight's best: {max(tab)}, same as {string(max(tab))} via string().
Empty ledger, interpolated: "{max(empty)}".
Empty ledger, via string(): {string(max(empty))}.
-> END
```

```text
Tonight's best: some(7), same as some(7) via string().
Empty ledger, interpolated: "".
Empty ledger, via string(): none.
```

`some(7)` prints identically both ways — the top line never diverges. The
bottom two lines are the same `none` value, read through the two
consumers: interpolated, it vanishes (the quotes above are there so an
empty result is visible on the page, not part of the rendering rule);
through `string()`, it still spells out `"none"`.

The forgiveness is cut by **position**, not by type or dialect: it only
ever applies to an interpolation's own **final**, top-level value. Compose
an Option with anything else — `{mood.first() + 1}`, an Option operand in
concatenation — and `Option<T> ≠ T` strictness still holds; that stays the
ordinary type error from [`Option<T>` is not `T`](#optiont-is-not-t), never
silently forgiven. And the forgiveness never destroys information: every
`None`-render is traceable in the transcript (the runtime's append-only
output log records the real `Value::OptionVal(None)`, not the empty text
it happened to render as), so tooling built on the transcript can always
tell "a `None` rendered here" apart from "nothing was ever emitted here."

Two loose threads, named rather than pretended away: an
always-`None`-interpolation lint (catching a slot that can *only* ever
render blank) and how choice text and tags should treat a forgiven `none`
(an accidentally blank choice differs from an author's deliberate `* []`)
are both still open questions, not yet answered by an implementation.

## Option and the collections

You've already met most of this chapter in [Collections](literals.md)
without the theory: half its verb table returns `Option<T>`, its
`when-the-world-doesn't-have-one` section is this doctrine in miniature,
and its empty-literal rule is E107's twin (a value born without a type must
be told one at birth). What this chapter adds is the frame those verbs sit
in — why `get` and `m[k]` *both* exist (a question and a claim, not a
redundancy), why `pop` on empty is `none` while `remove_at` out of bounds is
a fault (absence versus a wrong claim), and why none of these answers will
ever swap categories: the fault-vs-absence line is a ruling, not a
convention.

The two chapters that follow lean on this one harder. Iteration's fn-value
verbs include `filter_map` — the Option-aware mapper that transforms and
drops `none`s in one pass — taught alongside the rest of that family
there. Ordering has to answer what `min`/`max` mean when the
elements themselves misbehave (NaN), and its dev/prod doctrine is the
other half of the fault story. Both chapters assume you read this one.

## Reference: the diagnostics in this chapter

| Code | Fires when | Policy |
|---|---|---|
| `E066` | an Option and its element type collide — the slot is `Conflicted` | strict (runtime type fault under gradual) |
| `E107` | a fresh un-annotated declaration initialized from a bare `none` | both dialects, both policies |
| `E116` | an `Option<T>` used as a condition — no truthiness | strict (runtime fault under gradual) |

The gradual-mode runtime backstops, for completeness: an Option reaching a
truthiness evaluation is the no-truthiness fault; an Option meeting a plain
value in `==`/arithmetic is a type fault; and the malformed-question faults
(`find` on a non-string, `min` on a non-array or unorderable elements,
`get` on a non-map) fire in every mode — those were never absence.

## Where this is ruled

- **The Option package and the absence doctrine** ("a fault says 'your
  program is wrong'; Option says 'the world didn't have one'") —
  `docs/stdlib-spec.md` §1.1/§1.4/§1.6; decision log 2026-07-18
  ("Option<T> pulled forward as a compiler-known builtin; the absence
  doctrine").
- **The verb flips** (`find`/`index_of`/`first`/`last`/`min`/`max`/`pop`/
  `get` → `Option`; the sentinels dying unshipped) — `docs/stdlib-spec.md`
  §§3–5; same 2026-07-18 ruling; NS-A1 implementation (#1107). `pick` and
  `non_empty` joined with NS-A6 (#1112) and NS-A5 (#1136).
- **F27: no truthiness** — decision log 2026-07-19 ("F27/F28 ruled");
  `docs/stdlib-spec.md` §1.6. Supersedes the briefly-shipped falsy-`none`.
- **F28: `string()`'s totality is forever; the interpolation boundary
  forgives `none`** — same 2026-07-19 ruling; the display-boundary
  forgiveness (position-cut, nested never forgiven, traceable) is Track
  B4, `docs/stdlib-spec.md` §1.6b, SHIPPED (issue #1463). It turned out
  not to need the native surface — interpolation and `Option<T>` already
  exist on the current brink dialect, so it landed as a `brink-runtime`
  display change (`docs/stdlib-sequencing.md`'s Wave B4 entry has the
  as-built note).
- **`x or default`** — part of the 2026-07-18 package ruling; the typing
  substrate follows finding F19 (`docs/stdlib-phase-c-findings.md`). Surface
  spelling landed on the native `.brink` frontend in B1 (issue #1460):
  `InfixOp::Coalesce`, distinct from the brink dialect's oracle-frozen
  `InfixOp::Or` (ink's boolean `||`).
- **The `as`-binding** — named as the post-B1 condition-position spelling
  by F27 (below), then ruled in full by the decision log's 2026-07-26
  entry "The `as` binding: one construct, both condition positions,
  `{if}` spelling" (immutable, typed `T` from `Option<T>`, scoped to the
  success arm, rebinding per iteration in `while`, whole-condition-only
  for v1). Landed on the native `.brink` surface in B1b (issue #1475).
  Choice-guard `as` was ruled separately the same day
  ("Choice-guard `as` un-deferred: capture-at-presentation, by-value
  (COW), rides v6") and now lands too (issue #1508): the guard's binding
  captures into the same frame slot the pending choice's thread-fork
  snapshot already carries across selection — no new wire-format field
  needed (`E146`, "not yet supported", is retired).
- **Bare `none` needs a type from context** — `docs/stdlib-spec.md` §1.4;
  E107's declaration rule (#1107).
- **`Option<T>` in the static type language** — `docs/stdlib-spec.md`
  §1.4. Originally inferable-only, with annotatability slated for the
  native surface; landed generally instead as part of the 2026-07-27
  type-name conformance sweep (issue #1552) — `Option<T>`/`Weighted<T>`
  are annotatable on the ink/brink dialect today, not just the native
  frontend.
