# Values & Types

The Last Light inn charges by the night, and the innkeeper does not haggle:

```ink
VAR gold: int = 12
VAR room_rate: float = 3.5

The innkeeper chalks the rate on the slate: {room_rate} coins a night.
With {gold} coins, you can stay {nights_affordable(gold, room_rate)} nights.
-> END

=== function nights_affordable(purse: int, rate: float): int ===
~ return int(float(purse) / rate)
```

```text
The innkeeper chalks the rate on the slate: 3.5 coins a night.
With 12 coins, you can stay 3 nights.
```

Everything in that scene has a type: `gold` is an `int`, `room_rate` is a
`float`, `nights_affordable` takes one of each and gives an `int` back. The
compiler checked all of it before the story ran — the division that needed
both sides to be floats, the conversion back down to whole nights, the
interpolations. If any of it had been wrong, you'd have gotten a compile
error naming the exact spot, not a story that quietly printed nonsense.

That checking posture — **strict by default, inferred almost everywhere,
declared at the edges** — is the subject of this chapter.

> **Current spelling** — examples in this chapter compile in today's brink
> dialect: functions are ink-style `=== function … ===` knots and collection
> literals carry the `#[…]`/`#{…}` sigils. The ruled native `.brink`
> spellings — `fn` declarations, bare `[…]` literals, `[T]`/`[K: V]` type
> notation — arrive with the native frontend, and this chapter's examples
> will be respelled then.

## One checker, two policies

Every brink-dialect project has a type checker running underneath it,
whether or not it ever shows up. `types = strict` and `types = gradual` are
two policies over the **same** checker — not two type systems — so the
policy never changes what a program *means*, only whether the compiler
insists on proving more about it before it's allowed to run.

**Strict is the language's posture.** Since the 2026-07-19 typing-posture
ruling, a brink-dialect project with no `types` setting is strict; the
native `.brink` surface, when it lands, is strict-*only*. You opt out per
project, in [`brink.toml`](../project-config.md):

```toml
[project]
dialect = "brink"
types   = "gradual"   # explicit opt-out; omitting `types` means strict here
```

**Gradual is the compat floor, and it is permanent.** It is the strict-ink
dialect's default forever — the mode every plain-ink project is in. Types
are still inferred internally for tooling's benefit (hover, inlay hints,
advisory diagnostics), but nothing you write is *required* to resolve to a
concrete type: a slot the checker can't pin down stays `Unknown`, and
`Unknown` defers to the runtime's usual coercion behavior, unchanged from
how ink has always worked. Adding type annotations under gradual doesn't
opt you into anything stricter by itself.

Turning strict on changes three things, and only these three:

- An `Unknown` that would otherwise have escaped inference becomes a
  **compile error** (`E065`, "annotate or restructure") instead of a silent
  fallback.
- The coercion lattice narrows (see below) — most cross-type operations
  that gradual mode quietly tolerates become errors.
- Every variable becomes mono-typed, and collection element types must
  unify per collection (`#[1, 2.0]` is fine — see the note on
  `int → float` below — but `#[1, "a"]` is a compile error).

Strict mode requires the brink dialect (its annotation syntax is dialect
extension syntax, same as blocks and collection literals). Asking for
`types = strict` under `strict-ink` is a targeted config error, not a
silent no-op:

```text
error[E064]: types = strict requires dialect = brink — strict typing's
annotation syntax is a brink-dialect extension (docs/typed-mode-spec.md §1);
set `dialect = brink` or drop back to `types = gradual`
```

The oracle-anchored strict-ink subset — the plain-ink corpus this whole
compiler is validated against — is untouched by construction: a strict-ink
project always resolves to gradual, so strict typing only ever applies to a
project that has already opted into the brink dialect.

## The value kinds

Every value in a running story is one of a small, closed set of kinds. The
first four are the ones every scene touches:

- **`int`** — whole numbers: gold coins, visit counts, dice pips. Ink's
  native integer, 32-bit.
- **`float`** — fractional numbers: rates, weights, distances.
- **`bool`** — `true`/`false`. Conditions want one (with one deliberate
  exception — see the visit-count idiom below).
- **`string`** — text as a value: names, keys, anything you compare or
  store rather than merely print.

Beyond the scalars: **`divert`** (a knot/stitch target held as a value,
`-> market` in value position), **arrays and maps**
([Collections](literals.md) and [Indexing & Mutation](indexing.md)),
**`LIST`s** (ink's flag-set type,
nominal per declaration), **structs** (declared shapes, below), **function
values** ([Function Values](function-values.md)), **ranges**, the
**numeric tower** (`vec2`…`mat4`), and **handles** (host-owned resources,
typed by the engine's manifest).

When you write a type — in any annotation position — the names are
lowercase nominals:

| Written as | Meaning |
|---|---|
| `int`, `float`, `bool`, `string` | the scalar kinds |
| `divert` | a divert target as a value |
| `void` | return position only: "this function returns nothing" |
| `list<L>` | `L` names a declared `LIST` |
| `array<T>`, `map<K, V>` | typed collections |
| `fn(T…): R` | a function value ([Function Values](function-values.md)) |
| `vec2` `vec3` `vec4` `quat` `mat2` `mat3` `mat4` | the numeric tower |
| `handle<K>` | `K` names a handle kind from the host manifest |
| a declared `STRUCT` name | that struct's shape |

A name outside this vocabulary is `E061` ("`…` is not a recognized type"),
which lists exactly the table above. One kind is deliberately absent:
`Option[T]` appears in inferred types, diagnostics, and the
[Standard Library](stdlib.md)'s signatures (`find`, map `get`), but it is
display notation — you cannot write it in an annotation today, and a bare
`none` with no surrounding context to type it is its own error (`E107`,
"bare `none` needs a type from context").

## Inferred inside, declared at the edges

You will write far fewer annotations than the previous sections might
suggest, because the compiler reads your function bodies and works
signatures out bottom-up through the call graph:

```ink
-> ledger

=== function tab(nights) ===
~ return nights * 4

=== ledger ===
Three nights at four coins each: {tab(3)} coins.
-> DONE
```

```text
Three nights at four coins each: 12 coins.
```

`tab` carries no annotations, and under strict that's fine: the body
multiplies its parameter by a whole number, and that's evidence enough —
inference settles the signature as `(int) -> int` on its own. Mutually
recursive helpers are solved together as a group, and each definition gets
exactly one concrete signature, never a generic one.

Note where the evidence came from: **the body, only ever the body.** The
call `tab(3)` is *checked against* the settled signature; it never feeds
into it. This directionality is deliberate — a definition's meaning can't
be changed at a distance by whoever happens to call it — and it has one
consequence you'll meet in practice: a parameter whose body-uses are all
type-neutral (interpolation, passing it along to another unconstrained
slot) has no evidence to settle on, no matter how plainly its call sites
type it. That's what the `Unknown` escape below is about.

**Annotations are required at exactly one place: boundaries.** A boundary
is any signature the compiler can't see through from the call graph alone —
host-callable entry points, externals crossing to the engine, anything
whose callers aren't all visible. Internal helper functions never require
an annotation. There is no separate "you forgot to annotate a boundary"
diagnostic; the requirement enforces itself through inference: a boundary
parameter no body-use pins down escapes as `Unknown`, and strict mode turns
that escape into `E065` at the definition — annotate it and the error goes
away.

The practical corollary: an inferred signature ripples to its callers when
the body changes. That's an accepted, on-the-record cost — a body bug can
surface as an error at a caller rather than at its own definition — in
exchange for never forcing annotations onto code that doesn't cross a
boundary.

## Annotations

There is exactly one way to write a type: `name: type` after the thing it
types, and `): type` in a function header's return position. It works on
parameters, return types, `VAR` and `CONST` declarations, and `~ temp`
ascriptions:

```ink
VAR gold: int = 100
CONST RATE: float = 1.5

~ temp name: string = "hero"
~ gold = heal(gold, 10)

{name} has {gold} gold at rate {RATE}.
-> END

=== function heal(hp: int, amount: int): int ===
~ return hp + amount
```

```text
hero has 110 gold at rate 1.5.
```

Annotations are optional almost everywhere — the ones on `heal` above are
documentation more than necessity, since inference would have found the
same signature. But an annotation is never *just* a comment. It is a
**firewall**: the declared type is what callers see, whatever the body
does, and the checker then verifies the body against the declaration. When
the two disagree, that's `E063` — "annotated type `string` disagrees with
the type inferred from usage (`int`)":

```ink,error(E063)
-> tally("a quiet night")

=== tally(count: string) ===
{count > 2:
    A crowd tonight.
}
-> DONE
```

Under `types = gradual`, `E063` stays a warning — advisory seasoning.
Under `types = strict` it is promoted to a hard error: a signature that
lies about its body is exactly the kind of latent bug strict mode exists to
catch.

A function that returns nothing annotates `void` (or simply never
`return`s a value — inference treats the two identically). Assigning the
result of a `void` call is a strict-mode error, `E067` ("`f` returns void —
its result cannot be assigned"): there's nothing there to assign.

## When inference can't answer: Unknown and Conflicted

Strict mode's two signature errors are worth telling apart, because they
ask for different fixes.

**`Unknown` means inference ran out of evidence.** Nothing the body does
pins the slot down — and note that printing a value is *not* evidence,
because interpolation accepts every type. This fails with `E065`,
"`serve`'s parameter `dish` escapes strict inference as Unknown — annotate
or restructure":

```ink,error(E065)
-> serve

=== serve(dish) ===
The innkeeper slides {dish} across the bar.
-> DONE
```

The fix is in the message: either annotate (`=== serve(dish: string) ===`
— the annotation supplies exactly the fact inference couldn't find) or
restructure so the body genuinely uses the value.

**`Conflicted` means the body disagrees with itself.** The slot isn't
unconstrained — it's over-constrained, used as two irreconcilable types.
This fails with `E066`, "`haggle`'s parameter `offer` is Conflicted under
strict types — its uses disagree on its type":

```ink,error(E066)
-> haggle

=== haggle(offer) ===
{offer > 10:
    The trader whistles.
}
{offer == "generous":
    He bows.
}
-> DONE
```

No annotation can fix a `Conflicted` slot — declaring `offer: int` doesn't
make `offer == "generous"` sensible; it just moves where the contradiction
is reported. The only fix is to make the body agree with itself.

Under gradual, both cases fall back silently to runtime coercion behavior.
That's the whole policy difference in one sentence: gradual defers these
questions to the runtime; strict refuses to compile until they're answered.

## The coercion lattice, under strict

Gradual mode's coercions are exactly what ink has always done. Strict mode
narrows the lattice to one rule and two escape hatches.

**`int → float` is the one implicit, directional promotion.** An `int` is
welcome anywhere a `float` is expected — `#[1, 2.5]` is a well-typed
`array<float>`, `VAR rate: float = 1` is legal, and an `int` argument
promotes to match a `float` parameter:

```ink
-> weigh

=== function heft(w: float): string ===
{ w > 2.0:
    ~ return "heavy"
- else:
    ~ return "light"
}

=== weigh ===
The innkeeper's ledger calls it a {heft(3)} purse.
-> DONE
```

```text
The innkeeper's ledger calls it a heavy purse.
```

There is no `float → int` direction; that would silently lose precision.

**Everything else is explicit**, through the pure conversion intrinsics
`int(x)`, `float(x)`, and `string(x)`:

```ink
VAR fare: float = 2.5

~ temp coins: int = int("12")
You hand over {coins} coins; the ferryman counts {float(coins) * fare} in value.
-> END
```

```text
You hand over 12 coins; the ferryman counts 30 in value.
```

`int` and `float` accept numbers, bools, and strings (a string is parsed;
a string that doesn't parse is a turn-terminating fault, never a silent
zero). Anything outside that domain — a divert, a `LIST`, a collection —
is a compile error under strict (`E078`) and a runtime fault under
gradual. `string(x)` accepts every value and never fails: it's the same
display form interpolation uses.

**Interpolation is universal, not a coercion.** `{x}` accepts every type
under both policies — display was never part of the type lattice to begin
with.

## The idiom that survives strict: visit counts in condition position

Ink's oldest idiom is checking a knot's visit count directly in a
condition:

```ink
-> market ->
{market: The stalls are familiar now.|A first look at the market.}
-> END

=== market ===
Fruit, dice, gossip.
->->
```

```text
Fruit, dice, gossip.
The stalls are familiar now.
```

`market`'s visit count is a plain `int`; used bare in condition position,
nonzero means true. Nothing about strict typing touches this — it's scoped
deliberately: **condition position only**. Elsewhere, an `int` used where a
`bool` is expected is still an error under strict (`VAR ready: bool = 3`
never becomes legal), because that's not the idiom being preserved — this
is. A
strict mode that broke visit-count conditionals would be unusable for real
ink content, so this is a floor: turning strict on should produce errors
only where types genuinely conflict, never on ordinary visit-count logic.

One neighboring idiom does **not** survive, on purpose: an `Option` in
condition position. `{mood.first(): …}` is not "is there a first mood" —
`Option[T]` has no truthiness, ever. The condition-position error (`E116`)
tells you the honest spelling: test `== none` / `== some(x)` explicitly.
A fault says "your program is wrong"; `none` says "the world didn't have
one" — and a bare truthiness test blurs exactly that line.

## Collections and the empty-literal rule

Under strict, element types unify per collection: every element in an
`#[…]` (after the `int → float` join) must agree on one type, or the
collection's type is `Conflicted` and the binding holding it fails with
`E066`:

```ink,error(E066)
-> count_loot

=== count_loot ===
~ temp loot = #[1, "pearl"]
{len(loot)} treasures.
-> DONE
```

An empty literal (`#[]`, `#{}`) takes its type from surrounding context —
an already-typed binding, a typed argument position. If nothing constrains
it, that's an `Unknown` escape (`E065`): annotate the binding.

```ink
-> pack

=== pack ===
~ temp satchel: array<int> = #[]
~ push(satchel, 3)
{len(satchel)} item in the satchel.
-> DONE
```

```text
1 item in the satchel.
```

Map keys type as the built-in key domain (`int`, `string`, or `bool`) as a
single key sort, not a general union — brink has no user-facing union
types in this version. The full indexing and mutation contracts live in
[Indexing & Mutation](indexing.md).

## Structs

> This section will move to its own chapter (structs, enums, and flags)
> as the book's reorganization proceeds; it lives here until that chapter
> exists.

Structs are a closed-shape, flat-field value type. Declaring one mirrors
how you construct one: the `STRUCT` body is the same braced shape as a
construction literal, with types where the construction literal has
values.

```ink
STRUCT Point = #{
    x: float,
    y: float,
}

VAR p = 0

~ {
    p = Point#{x: 1.0, y: 2.0}
    p.x = 9.0
}

{p.x} {p.y}
-> DONE
```

```text
9 2
```

Field access (`p.x`) resolves through the same fallback rule brink uses
for direct-call syntax elsewhere: ink's own static dotted paths
(`knot.stitch`, `List.Item`) are tried first and win; `.x` is field access
only once the head resolves to a plain variable. Under strict the shape is
known at compile time, so field reads and writes compile to static
offsets; under gradual, an `Unknown`-typed head defers the lookup to
runtime, by name.

**Structs work under either policy, with different failure timing for the
same mistake.** A construction literal is checked against the declared
shape — missing a field is `E069`, supplying an undeclared one is `E070`,
a field value of the wrong type is `E071`, and naming the same field twice
is `E084` (under both policies — a duplicate's initializer is never
silently dropped). Under strict these are compile errors; under gradual
the malformed construction is a runtime fault that ends the story turn
instead of producing a half-built value. Neither policy silently accepts a
malformed construction — only *when* the mismatch is caught differs.

A construction literal *is* a legal `VAR`/`CONST` declaration default, so a
struct-typed global can be given its real starting value where it is
declared. The literal has to be well-formed there: a declaration default is
baked into the compiled story, with no runtime construction step left to
fault at, so a mismatched one is a compile error under either policy
(`E075`) rather than a gradual-mode runtime fault. One thing to keep in
mind either way: initializers in a construction literal always evaluate in
the order *you wrote them*, never the shape's declared order.

A struct passed to or returned from a function behaves like any other
value: the callee gets its own independent copy, and mutating it never
reaches back to the caller's — the same value semantics arrays and maps
have.

## Types at the seams

Inside one project, inference sees everything. At the project's edges it
can't, and each edge has its own answer:

- **Engine functions** (`EXTERNAL`) have bare, untyped parameters in ink
  source — their types come from the host's binding manifest (see
  [External Functions](../embedding/external-functions.md)). A registered
  external whose manifest types resolve is checked at every call site like
  any other function; an external with no registered signature stays
  deliberately unchecked. Under strict, a manifest type that *fails* to
  resolve is a real `E065` escape at the declaration — the seam must be
  typed, or it's an error, never a shrug.
- **The ink seam** — *planned.* When the native `.brink` surface lands,
  ink-authored symbols will enter native code as `Unknown`, and strict's
  existing escape rule does the rest: annotate at the seam, exactly the
  boundary doctrine above. Today there are no mixed ink/brink trees, so
  this seam exists as a ruling, not yet as tooling; the compat posture is
  described in [Conformance](conformance.md).

## Reference: the diagnostics in this chapter

| Code | Fires when | Policy |
|---|---|---|
| `E061` | annotation names an unrecognized type | both |
| `E063` | annotated type disagrees with the type inferred from usage | warning under gradual, error under strict |
| `E064` | `types = strict` without `dialect = brink` | config |
| `E065` | a type escapes strict inference as `Unknown` | strict |
| `E066` | a type is `Conflicted` — its uses disagree | strict |
| `E067` | assigning the result of a `void` function | strict |
| `E069`/`E070`/`E071` | struct construction missing / extra / mistyped field | strict (runtime fault under gradual) |
| `E075` | struct construction literal in a `VAR`/`CONST` default doesn't match its declared shape | both |
| `E078` | `int()`/`float()` argument outside the numeric+bool+string domain | strict (runtime fault under gradual) |
| `E084` | duplicate field in a struct construction literal | both |
| `E107` | bare `none` with no type from context | both |
| `E116` | `Option[T]` used as a condition — no truthiness | strict (runtime fault under gradual) |

## Where this is ruled

- **Typed mode** — `docs/typed-mode-spec.md` §§1–5 (policy, inference,
  annotation syntax, coercion lattice, collections); §6 (structs).
- **Strict as the default; native strict-only** — decision log 2026-07-19,
  "Typing posture ruled" (NS-A9); the dialect-keyed default in
  `resolve_type_policy`.
- **Option, absence, and no truthiness** — `docs/stdlib-spec.md` §1
  (Phase A postures; F27 ruled 2026-07-19).
- **Struct construction order and duplicate fields** — decision log
  2026-07-14 (#675/#676).
