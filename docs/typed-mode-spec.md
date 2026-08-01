# Typed mode spec — strict types, inferred internally, declared at boundaries

Status: design round 2026-07-12 (#605), four core rulings by maintainer
(sections marked **RULED**). Sections marked **PROPOSED** are this
document's concrete surface proposals — ratified by merging the spec PR,
revisable at review. Companion to `docs/value-model-spec.md` (semantics),
`docs/t1b-surface-spec.md` (dialect gate), and the language-facility
doctrine (decision log 2026-07-12).

## 1. What typed mode is — RULED

A **project-level policy over one shared checker**, not a second checker:

> **Amended 2026-07-19 (NS-A9, dialect-keyed default ruling):** the
> unset-`types` default is now **dialect-keyed**, resolved by one seam
> (`brink_analyzer::resolve_type_policy`): the **brink dialect defaults to
> `types = strict`**; the strict-ink dialect defaults to `types = gradual`
> (where `strict` remains the E064 config error it always was). An explicit
> `types` choice — CLI flag, `brink.toml`, or API call — always wins over
> the dialect-keyed default. Gradual is not removed: it remains the floor
> and the strict-ink default, and any brink project can opt back out with
> `types = gradual`; it is simply no longer the brink dialect's *implicit*
> default. References to "gradual (default)" below predate this amendment.

- `types = gradual` (the strict-ink dialect's default): the 2026-07-10
  architecture unchanged —
  `Unknown` unifies with anything and defers to runtime coercion.
  Annotations are optional seasoning. This remains the floor forever;
  non-programmer-authored projects never see typed mode.
- `types = strict` (the brink dialect's default since 2026-07-19):
  `Unknown` escaping inference is a **compile error**
  ("annotate or restructure"); the coercion lattice narrows (§4);
  variables are mono-typed; collection element types must unify (§5).

Config surface: project-level, mount-time, alongside the dialect —
authoring-time only, never in `.inkb`, per the dialect precedent. Strict
mode requires the brink dialect (its annotation syntax is extension
syntax). The oracle-anchored strict-ink subset is untouched by
construction.

## 2. Inference — RULED

**Monomorphic HM per call-graph SCC** (Haskell binding-group style):

- Parameter types are inferred from **uses inside the body**; signatures
  derive bottom-up through the call graph; recursive SCCs solve by
  fixpoint. Call-site-driven inference is forbidden — `infer_body(A)`
  reads only `signature(B)`, preserving the phase-0 signature firewall
  and salsa incrementality.
- **User code is monomorphic in v1**: every unification variable must
  resolve to a concrete type per definition. The polymorphic stdlib
  (`len`/`contains`/`insert`/`remove` over arrays and maps) is handled
  by intrinsic typing rules per the doctrine (rules attach to resolved
  builtin definitions).
- No overloading, no typeclasses, one directional numeric coercion —
  constraint solving stays near-linear (the Swift blowup ingredients are
  absent by construction).

**Annotations are required at boundaries only** (the ruled fork):
host-callable functions, entry points, and `#fn` targets that cross to
the engine — exactly where effect rows freeze (T2 symmetry). Internal
helpers never require an annotation. Accepted cost, on the record:
inferred signatures ripple to callers on body edits (salsa early-cutoff
contains no-change), and a body bug can surface at a caller.

**RULED (issue #1763): a block-bodied lambda's own locals are outside
the enclosing def's strict frame** — neither `Unknown`-escape-checked
against it nor ascription-exemptable there. This falls out of #1750's
frame-boundary fix: `InferPass::infer_lambda` snapshots and restores
the enclosing def's `locals` around a lambda body, so a name declared
only inside a lambda never becomes a key in the enclosing def's
`body_types.locals` — there is nothing for the enclosing frame's
`Unknown`-escape check to see, ascribed or not. A lambda gets its own
strict-checked frame only if/when a future change gives it one (its
own `BodyTypes`, run through the checker in its own right rather than
folded into the enclosing def's).

## 3. Annotation syntax — RULED

**Inline types, brink-dialect-gated.** This revises the #473 ruling: the
`#@` channel keeps its other tenants (`#@local` etc.) but **never
carries types**. One way to write a type.

```ink
=== function heal(ref hp: int, amount: int): int ===
VAR gold: int = 100          // optional anywhere; required only at boundaries
~ temp name: string = who    // ascription available, rarely needed
```

**PROPOSED grammar details** (ratify at review): `name: type` after
params and declarations; `): type ===` return position; `void` for
no-return functions (assigning a `void` call is an error in strict
mode). Primitives are lowercase, every other type name is Uppercase
(issue #1552, `docs/decision-log.md` 2026-07-27 "Type-name surface
ruled" — enforcement of this spec's own 2026-07-19 casing partition):
`int`, `float`, `bool`, `string`, `divert`, `List<L>` (nominal per LIST
declaration), `Array<T>`, `Map<K, V>`, `Option<T>`, `Weighted<T>`,
`fn(T…): R` (function values, for the unfrozen T1c), `Handle<K>`
(T1d-2, docs/t1d-spec.md §3 — this spec's first amendment: `K` names a
handle kind declared in the external manifest's host semantic-type
vocabulary — not an ink-source declaration like `LIST`/`STRUCT`), plus
declared struct names.

**RULED (issue #1591): "the body" of a function/knot/stitch, for
inferred-void and return-value-fall-through purposes, is the def's own
block *plus* its stitches** — a stitch is reachable by fall-through and
is part of the same definition's execution, not a separate callable.

## 4. Coercion lattice in strict mode — PROPOSED

- `int → float`: implicit, directional (the one ink numeric promotion).
- **Condition-position int truthiness stays** (`{visited_knot: …}`,
  nonzero = true): this is core ink idiom and a strict mode that breaks
  visit-count conditionals is unusable. Scoped to condition position
  only; `bool b = 3` remains an error.
- Everything else explicit: pure conversion intrinsics `int(x)`,
  `float(x)`, `string(x)` (typing rules per the doctrine).
- Interpolation/printing (`{x}`) accepts every type — display is
  universal, not a coercion.

## 5. Collections and the empty-literal rule — PROPOSED

Element types unify per definition: `#[1, 2.0]` is `Array<float>` (via
the int→float join); `#[1, "a"]` is an error pointing at structs.
`#[]`/`#{}` take their types from surrounding constraints within the
def; if unconstrained, that's an `Unknown` escape → annotate the
binding. Map keys type as the key domain (`int | string | bool` stays a
built-in key sort, not a general union — user-facing union types do not
exist in v1).

## 6. Structs — RULED to land with strict mode; surface PROPOSED

`Value::Record` per the 2026-07-11 reservation ruling: closed shape,
flat field array, interned shape id; all ratified value-model machinery
(COW, serialization, equality, sharing law) applies unchanged. Format
surface already reserved (V4 `StructShapes` section + field-op opcode
space).

**PROPOSED surface** (the genuinely new design — review this hardest):

```ink
STRUCT Point = #{                          // decl body MIRRORS the literal:
    x: float,                              // types sit where values go
    y: float,
}

~ temp p = Point#{x: 1.0, y: 2.0}          // construction: typed brace literal
~ temp x = p.x                              // field access
~ p.y = 3.0                                 // field write (RMW discipline)
```

- Declaration keeps ink's `NAME = …` decl convention and its body takes
  the same braced `#{…}` shape as the construction literal, with types
  in value position — declaration and usage rhyme (amended from a flat
  comma list, maintainer ruling 2026-07-13). Single-line form is legal
  for short structs; brink-fmt formats multiline bodies like blocks.
  Fields are typed (structs only exist meaningfully under strict mode,
  though gradual projects may use them dynamically).
- Construction reuses the sigil-literal shape with a leading shape name
  — self-identifying, gateable lexically.
- **Field access `p.x` uses resolution fallback** (same pattern as the
  T1c direct-call ruling): if the head resolves to a variable, `.x` is
  field access; ink's static dotted paths (knot.stitch, List.Item) are
  resolved first and win. In strict mode this is compile-time
  unambiguous; in gradual mode an `Unknown` head defers to a runtime
  field lookup with a fault on missing field.
- Missing/extra fields at construction: compile error (strict) /
  construction fault (gradual). Field offsets compile statically under
  strict (the performance payoff the structs ruling anticipated).
- **Construction-literal initializers evaluate in source order** — the
  order the author wrote them, left-to-right — never the shape's
  declaration order, under either policy (RULED 2026-07-14, decision-log
  "Struct construction literals: source-order evaluation, duplicate field
  is a compile error", issue #676). Shape order is purely a memory-layout
  concern for the compiled `RecordNew` push order; it never governs when
  an initializer's side effect fires.
- **A duplicate field in a construction literal is a compile error**
  (`E084`) under both `types = gradual` and `types = strict` — the
  repeated occurrence's initializer, including any side effect, is not
  silently dropped (same ruling, issue #675).

## 7. What strict mode checks in plain-ink content — RULED

A brink-dialect + strict project still contains ordinary ink (knots,
choices, `~` one-liners). Strict typing covers all of it — VARs,
function calls, conditions, interpolations — under the same lattice
(§4 keeps the idioms that make ink ink). Existing valid-ink projects
switching to strict should expect errors only where types genuinely
conflict (cross-type VAR reassignment, heterogeneous collections,
`Unknown` escapes), not on idiomatic visit-count logic.

All three named cases are now implemented and tested: heterogeneous
collections and `Unknown`/`Conflicted` escapes since #619 (TM-3);
cross-type VAR reassignment — including a plain `~ v = expr` against a
`VAR`/`CONST`'s declared type, `~ temp` declaration initializers and
ascriptions, and (issue #1877's own scope) an **unannotated** global's
initializer-literal-inferred type, not only an explicit `: type`
annotation — since #1877.

## 8. Effects touchpoint — RULED direction (T2 owns the detail)

Types and effects share the policy (*inferred internally, frozen at
boundaries*) and the machinery (per-def rows behind the signature
firewall; SCC fixpoints). Types land first and prove the skeleton;
the T2 round designs effect semantics on top of it. Asymmetry stands:
types may be gradual per project; effect rows are conservative-total
(unknown = "may do anything", never omitted).

## 9. Sequencing — RULED

**Types → T1c → effects.** Milestone **TM (typed mode)** inserts
between T1b and T1c:

1. **TM-1 checker substrate**: `signature`/`infer_body`/
   `type_diagnostics` queries, mono-HM per SCC, intrinsic rules for the
   stdlib — advisory-only (gradual policy) in both dialects. Oracle
   byte-identical; zero behavior change.
2. **TM-2 inline annotation syntax**: grammar/HIR/fmt/IDE for §3,
   brink-dialect-gated, feeding `signature()` (annotation = firewall).
3. **TM-3 strict policy**: `types = strict` config + `Unknown`-escape
   errors + §4 lattice + mono variables + §5 collection rules +
   boundary-annotation requirement.
4. **TM-4 structs**: §6 surface + `Value::Record` + shapes section +
   field opcodes (reserved space; VERSION stays 4) + static offsets
   under strict.
5. **TM-5 tail** (pump wave): corpus wing growth, book chapter
   ("Types"), IDE surfacing (inferred-type hover/inlay, boundary-
   annotation quick-fix), typed-LIR divergence hook exploration.

Then **T1c unfreezes** with `fn(T…): R` types from day one, then the
T2 effects round. Spine slices run as single reviewed, oracle-gated
agents; the tail as a pump wave — the T1b working method unchanged.

## 10. Where the strict pass is exercised — issue #1882

TM-3's checks only run where a corpus is actually compiled under
`types = strict`, and until #1882 the **native** golden corpus was not
one of those places. `tier1_native.rs` compiles every
`tests/tier1-native/<case>/story.brink` with `AnalysisOptions::default()`
(`dialect = StrictInk` → `Gradual`), so `strict::check` never saw
`.brink` source — while a real `.brink` project that sets
`dialect = "brink"` gets `Strict` from §1's dialect-keyed default. Every
native strict-typing bug found between #1849 and #1902 was a question
that corpus would have answered.

**Ruled: the native corpus is swept under strict, against a recorded
baseline** — `crates/internal/brink-test-harness/tests/
tier1_native_strict.rs`. It compiles every case under
`types = strict` and asserts the resulting `E063`/`E065`/`E066` set
matches a hand-classified table, failing in **both** directions (a new
finding needs triage; a finding that stops firing means a gap closed or
a check regressed).

Two things this deliberately is *not*:

- It is **not** a flip of `tier1_native.rs`'s own posture. That file's
  goldens stay on the default (gradual) compile, so the sweep can never
  turn a typing question into a transcript failure. `tier1_native_
  strict.rs` pins that separation with its own assertion.
- It is **not** licence to edit fixtures until the sweep is green.
  CLAUDE.md's rule stands: a check that trips on real corpus code means
  the check is wrong, or reality differs from this spec — flag it. The
  baseline is a worklist, and each row names whether it is a true
  positive or a checker gap.

The first sweep produced 37 findings across 7 of 14 cases, filed as
#1909 (UFCS method-call results type as `Unknown` where the direct-call
spelling is clean), #1910 (pure verb results and lambda-bound locals
type as `Unknown`), #1911 (`string + int` concatenation reports `E066`
on legal, running ink), and #1912 (`content`-typed parameters — value
reads type as `Unknown`, and `try_claim` synthesizes `string`
arguments the handler signature rejects). The rest are fixtures written
in gradual style, expected under §2 and §5.

Two findings land back on this spec rather than on the analyzer. §4's
coercion lattice does not say what `string + T` concatenation does at
all (#1911). And §2's "internal helpers never require an annotation"
promise is not what the corpus experiences: a helper whose body
genuinely constrains nothing (`fn bump(ref n, amount) { n = n +
amount; }`) is an `Unknown` escape — correct under "call-site-driven
inference is forbidden", but not what that sentence leads a writer to
expect. §2 should say "never require an annotation *when the body
constrains them*".
