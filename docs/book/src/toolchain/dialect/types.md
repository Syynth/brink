# Types

Every brink-dialect project has a checker running underneath it, whether or
not it ever shows up. `types = gradual` (the default) and `types = strict`
are two policies over the **same** checker — not two type systems — so
turning strict mode on never changes what a program *means*, only whether
the compiler insists on proving more about it before it's allowed to run.

## Gradual vs. strict

**Gradual** is the floor every project stands on, forever. Types are
inferred internally for tooling's benefit (hover, inlay hints, diagnostics),
but nothing you write is *required* to resolve to a concrete type. An
unannotated parameter that the body never pins down stays `Unknown`, and
`Unknown` unifies with anything — it defers to the runtime's usual
coercion behavior, unchanged from how brink has always worked. This is the
mode every existing project is already in; adding the dialect's type
annotations doesn't opt you into anything stricter by itself.

**Strict** is an opt-in, project-level policy (set alongside the dialect at
mount time — authoring-time only, never embedded in `.inkb`):

```text
dialect = brink
types = strict
```

Turning it on changes three things, and only these three:

- An `Unknown` that would otherwise have escaped inference becomes a
  **compile error** ("annotate or restructure") instead of a silent
  fallback.
- The coercion lattice narrows (see below) — most cross-type operations
  that gradual mode quietly tolerates become errors.
- Every variable becomes mono-typed, and collection element types must
  unify per collection (`#[1, 2.0]` is fine — see the note on `int → float`
  below — but `#[1, "a"]` is a compile error).

Strict mode requires the brink dialect (its annotation syntax is dialect
extension syntax, same as blocks and collection literals); asking for
`types = strict` under `strict-ink` is a targeted config error, not a
silent no-op. The oracle-anchored strict-ink subset — the plain-ink corpus
this whole compiler is validated against — is untouched by construction:
turning on strict typing is something *you* do to *your* project, never
something the compiler infers or defaults into.

## Annotations

Type annotations are **inline, brink-dialect-gated, and optional almost
everywhere**. There is exactly one way to write a type in brink: `name:
type` after a parameter or declaration, and `): type` in the return
position of a function header.

```ink
=== function heal(ref hp: int, amount: int): int ===
VAR gold: int = 100          // optional anywhere; required only at boundaries
~ temp name: string = who    // ascription, rarely needed
```

Type names are lowercase nominals: `int`, `float`, `bool`, `string`,
`divert`, plus `list<L>` (nominal per a `LIST` declaration), `array<T>`,
`map<K, V>`, and declared struct names. A function with no return value
annotates `void`; assigning the result of a `void` call is a compile error
under strict mode (there's nothing there to assign).

**Annotations are required at exactly one place: boundaries.** A boundary
is any signature the compiler can't see through from the call graph alone
— host-callable functions, entry points, `#fn` targets that cross to the
engine. Internal helper knots never require an annotation; their parameter
and return types are inferred bottom-up through the call graph instead
(monomorphic Hindley-Milner, one pass per call-graph strongly-connected
component). This is why the `heal` example above only *needs* the boundary
convention loosely — nothing stops you annotating an internal helper too,
it's just never mandatory.

The practical corollary: an inferred signature ripples to its callers when
the body changes. That's an accepted, on-the-record cost — a body bug can
surface as an error at a caller rather than at its own definition — in
exchange for never forcing annotations onto code that doesn't cross a
boundary.

## The coercion lattice, under strict

Gradual mode's coercions are exactly what brink has always done. Strict
mode narrows the lattice to:

- **`int → float` is the one implicit, directional promotion.** `#[1,
  2.0]` is a well-typed `array<float>` — the `1` promotes to join with
  `2.0` — and `VAR rate: float = 1` is legal for the same reason. There is
  no `float → int` direction; that would silently lose precision.
- **Everything else is explicit**, through the pure conversion intrinsics
  (`int(x)`, `float(x)`, `string(x)`).
- **Interpolation is universal, not a coercion.** `{x}` accepts every type
  under both policies — display was never part of the type lattice to
  begin with.

## The idiom that survives strict: condition-position int truthiness

Ink's oldest idiom is checking a knot or stitch's visit count directly in
a conditional:

```ink
{ market: You've been here before. | First time in the market. }
```

`market`'s visit count is a plain `int`; used bare in condition position,
nonzero means true. Nothing about strict typing touches this — it's scoped
deliberately: **condition position only**. Elsewhere, an `int` used where
a `bool` is expected is still an error under strict (`bool b = 3` never
becomes legal), because that's not the idiom being preserved — this is.

A strict-mode checker that broke visit-count conditionals would make
strict mode unusable for real ink content, so this is a floor, not a
convenience: every existing valid-ink project that turns strict mode on
should see errors only where types genuinely conflict (a `VAR` reassigned
across incompatible types, a heterogeneous collection, a real `Unknown`
escape) — never on ordinary visit-count logic.

## Structs

Structs are a closed-shape, flat-field value type — `Value::Record` in the
runtime's value model, with the same COW/equality/sharing guarantees every
other collection has. Declaring one mirrors how you construct one: the
`STRUCT` body is the same braced shape as a construction literal, with
types where the construction literal has values.

```ink
STRUCT Point = #{
    x: float,
    y: float,
}

VAR p = 0

~ {
    p = Point#{x: 1.0, y: 2.0}   // construction: typed brace literal
    p.x = 9.0                     // field write (read-modify-write)
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
only once the head resolves to a plain variable. Under strict mode this is
fully resolved at compile time — the compiler knows `p`'s shape, so field
offsets compile to static reads/writes. Under gradual mode, an `Unknown`-
typed head defers field lookup to runtime, by name.

**Structs work under either policy, with different failure timing for the
same mistake.** Missing or extra fields at construction are:

- a **compile error** under strict (`E069` for missing fields, `E070` for
  extra ones — the compiler knows every declared shape, so it checks
  construction literals against it directly), and
- a **construction fault at runtime** under gradual (the shape simply
  isn't buildable from what was given, so the story turn ends rather than
  silently dropping or inventing a field).

```ink
STRUCT Point = #{x: float, y: float}
~ temp p = Point#{x: 1.0}   // missing `y`
```

Under `types = strict`, this doesn't compile. Under `types = gradual`, it
compiles, and running it faults the turn instead of producing a
half-built `Point`. Neither policy silently accepts a malformed
construction — only *when* the mismatch is caught differs.

A struct passed as a function argument or returned from one behaves like
any other value: the callee gets its own independent copy, and mutating it
never reaches back to the caller's copy — the same value semantics arrays
and maps already have.

## Collections and the empty-literal rule

Under strict, element types unify per collection: every element in an
`#[…]` (after the `int → float` join) must agree on a single type, or it's
a compile error pointing at exactly the disagreeing elements. An empty
literal (`#[]`, `#{}`) takes its type from the surrounding context within
its definition — assigned to an already-typed binding, passed as a typed
argument — and if nothing constrains it, that's an `Unknown` escape:
annotate the binding it's assigned to.

Map keys type as the key domain (`int | string | bool`) as a single
built-in key sort, not a general union — brink has no user-facing union
types in this version.
