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

**RULED (issue #1763; superseded in part by issue #1770): a
block-bodied lambda's own locals are outside the *enclosing def's*
strict frame** — neither `Unknown`-escape-checked against it nor
ascription-exemptable there. This falls out of #1750's frame-boundary
fix: `InferPass::infer_lambda` snapshots and restores the enclosing
def's `locals` around a lambda body, so a name declared only inside a
lambda never becomes a key in the enclosing def's `body_types.locals`
— there is nothing for the enclosing frame's `Unknown`-escape check to
see, ascribed or not.

Issue #1770 gave a lambda its own strict-checked frame — a lambda's
own params and body-declared temps are now escape-checked in their own
right (`BodyTypes::lambda_escapes`, a flat, cumulative vector fed
straight from `InferPass::infer_lambda`'s own walk and re-emitted by
`check_def` under the enclosing def's own label), not by giving the
lambda a `BodyTypes`/`DefinitionId` of its own — a lambda still has
none at inference time (`hir::stamp_container_ids` runs only after
analysis; see issue #1727). A lambda's own *return*-type slot stays
outside this frame, deliberately: issue #1994's
`LambdaAnnotationMismatch` (`E174`) already owns a materially
different, eager check for a written return annotation disagreeing
with the body, and a second, gradual escape check on the identical
slot would double-report the same fact under a different code.

**RULED (issue #1912): a parameter's own annotation is visible to the
body walk at *pure read sites*** — positions that consume the value and
produce no counter-evidence about it. `return <param>` is one, joining
the `infer::body::InferPass::or_own_annotation` fallback #1168 already
applied to `some(x)`, `get(m, k)`'s return shape, and a `for` loop's
iterable. So `fn passthru(t: content) { return t; }` exports `content`,
not `Unknown` — the return type is exactly the annotated parameter type,
and the annotated-return twin was already clean. The gap this closes was
filed against `content` but was general to every resolvable annotation.

The firewall this must not dissolve, and does not: the fallback overlays
an `Unknown` **only**. A parameter the body genuinely constrains still
exports its own independent derivation, so `annotations::mismatches`
(`E063`) keeps comparing two derivations rather than the annotation
against itself, and a contradictory body still comes out `Conflicted`
(`E066`). Correspondingly, *evidence-producing* positions — `infer_infix`'s
comparison and arithmetic operands, an intrinsic's sibling-argument
`observe` (#1168's own w65 correction) — deliberately never consult the
fallback: there, an annotation flowing into the walk would become body
evidence for a *second* slot and silently discard that slot's own
annotation. An unascribed temp merely copying an annotated parameter
(`let v = t;`) likewise does **not** inherit the annotation transitively;
the boundary is "annotated param / ascribed temp", read directly.

**RULED (issue #1941): the same pure-read-site fallback extends to a
lambda's own value position** — `infer::body::InferPass::infer_lambda`'s
block-body tail (`LambdaBody::Block`) and sole expression
(`LambdaBody::Expr`) are both structurally identical to `return <param>`
above, one syntax form over: `|t: content| { t }` and `|t: content| t` now
export `content`, not `Unknown`, exactly like `fn f(t: content) { return t; }`
does. #1938 fixed only the `fn`/`flow` `return` position; the lambda's own
value-position reads were the gap this closes.

One wrinkle unique to a lambda: a plain `fn`/`flow` gets its `annotated`
fallback map seeded from `def.params` for free at pass-creation time
(`infer_def_body`'s `new_pass` call), but nothing ever seeded a *lambda's*
own param annotations into that map at all — `infer_lambda` only ever
*shadowed* (cleared) whatever an enclosing same-named local's annotation
left behind, so `or_own_annotation` had nothing to fall back to even after
being wired up at the read site. The fix seeds `self.annotated` with the
lambda's own resolvable param annotations for the duration of its own body
walk, restored via the same shadow/restore mechanism issue #1910 already
uses for every other frame-scoped map this function touches — an enclosing
same-named local's own annotation is exactly as protected as it was before.

**This seed's reach is the whole body walk, not only the tail/expr value
position.** `self.annotated` is read by `own_annotation`'s bare-name
fallback at *every* `or_own_annotation`/`annotated_callee_ty` consumer
reachable while the walk runs — an intrinsic's argument-position overlay
(`some(t)`), a `for` loop's iterable, and a direct-call callee's own
annotated type (`cb(1)` for a `cb: fn(int): int` param) all resolve through
the same seed, exactly like a `fn`/`flow`'s own `new_pass`-time seed already
covers its whole body, not only its `return`s. Only the value-position
tail/expr reads were previously *wired up* to consult the fallback (#1941's
own read-site fix); the seed that feeds them was never scoped to those two
sites alone.

One exclusion the seed must respect: a param name the lambda's own body
*re-binds* (a fresh `TempDecl`/`if`/`while`/`for` binding of the same
spelling, e.g. `|t: int| { let t = "a"; t = "b"; t }`) is never seeded.
`check_declared_assign_target`'s `SymbolKind::Temp` arm reads this same
bare-name-keyed map to find a Temp's *declared* type for its own mismatch
report — it has no way to tell "the param's own annotation" apart from "a
fresh same-named local's own (absent) annotation" — so seeding the param's
type here would falsely report the fresh local's own unrelated assignment
as a mismatch against the shadowed param's type. `infer_lambda` computes
the body's own re-bound names (`lambda_own_binding_names`, already gathered
for the `locals`/`annotated` shadow above) and skips seeding any param name
that set contains.

The firewall holds identically: the fallback overlays an `Unknown` only, so
a lambda body that genuinely constrains its param still exports its own
derivation, and a lambda's own explicit return annotation
(`|t|: T { … }`) still overlays only when the tail/expr comes back
`Unknown` — unchanged by this fix.

**Distinct from the #1912 "no seed" scope note.** #1912's own scope-notes
comment records that a whole-body **seeding** approach was tried and
rejected for the `fn`/`flow` case: it seeded `self.locals` — the inference
lattice itself — and broke the "overlay, never seed" firewall load-bearing
for `E063`/`E066`, concluding "any future widening has to be per-read-site,
not a seed." This fix does not breach that firewall: it seeds
`self.annotated`, the read-site *fallback* map a `fn`/`flow` already
receives for free at `new_pass` time — never `self.locals` — so a lambda
body that genuinely constrains its param still produces its own
independent derivation in `self.locals`, exactly as before. Recorded here
so the two do not read as contradictory: different map, different
contract, same firewall intact.

**RULED (issue #1994, closing #1932, 2026-08-01): a lambda's own written
annotation governs its type unconditionally, and the checker errors
immediately when the body disagrees — deliberately NOT the same precedence
as a top-level `fn`/`flow`.** #1910/PR #1928 gave `infer_lambda` the same
posture `infer_def_body` already had (body-derived wins, annotation only
the `Unknown` fallback above), which was never itself ruled for a lambda —
it fell out as a side effect of reusing the `fn`/`flow` shape, and it meant
a *wrong* body derivation could silently override a *correct* written
annotation with no diagnostic anywhere (a standalone `let f = |k: int|:
int { "wrong" };`, never called, produced nothing at all).

This reconciles that gap against this section's own "annotation = firewall"
language (§9's phrasing for what feeds `signature()`) by splitting it in
two, on purpose:

- **`fn`/`flow`** (unchanged): the annotation is the *fallback* firewall —
  it fires only when the body itself leaves a slot `Unknown`, exactly as
  ruled above. A body that disagrees with the annotation still exports its
  own concrete/`Conflicted` derivation, and the disagreement is only ever
  the gradual, `[lints]`-configurable advisory `E063`
  (`annotations::mismatches`) — TM-3's call, not a hard failure.
- **A lambda's own written param/return annotation** (this ruling): the
  annotation *replaces* the body-derived type at that slot, full stop. A
  body-derived type that disagrees (when it resolves to anything concrete —
  `Unknown`/`Conflicted` never "disagrees", same guard
  `report_if_mismatched` uses) is recorded as an eager, `Error`-severity
  `E174` (`infer::body::InferPass::infer_lambda`, reported by
  `strict::check_lambda_annotation_mismatches`), raised at the lambda's own
  declaration — never deferred to wherever the lambda is later called, and
  never downgradable the way `E063` is.

The two are ruled to differ because a lambda is typically small and
locally scoped, and more likely annotated specifically to pin down what its
body should mean, than a top-level `fn`/`flow` whose annotation is
primarily a boundary contract for callers. **Scope is narrow and literal**:
this is the lambda's own `|p: T|`/`: R` syntax only — an enclosing
binding's declared type (`let f: fn(int): int = |k| …`) is not "the
lambda's own annotation" and does not reach this mechanism. It also does
not apply to a param the lambda's own body *re-binds* (`|t: int| { let t =
"a"; t = "b"; t }`): `narrowed_params` is bare-name-keyed off `self.locals`,
so once the body's own `TempDecl` shadows the param, there is nothing left
in this pass's bookkeeping that still distinguishes "the param's own
narrowing" from "the shadowing local's" — that one param falls back to the
unannotated `fn`/`flow`-style posture unconditionally (no diagnostic either
way), the same carve-out issue #1954 already made for the `self.annotated`
seed just above, for the identical reason.

An unannotated param/return is entirely unaffected by this ruling — #1910's
own fix stands exactly as it was: the body-derived type still wins over
nothing, because there is no written annotation to govern with.

**RULED (issue #2773): this is the CHECK-side counterpart to the
inference-side frame boundary ruled above — inside a lambda's own body,
strict-mode CHECK classification reads the lambda's own written `: T`
param annotations, and treats every other lambda-own binding as
unclassifiable there, never the enclosing def's same-named binding.**
The frame-boundary rulings above (#1763/#1770/#1912/#1941/#1994) all
concern *inference* (`infer::body::InferPass`, `BodyTypes::locals`
itself); this rule concerns the *CHECK* passes that read the already-
finalized `BodyTypes::locals` back out by bare name after inference is
done. `hir::visit::walk_expr`'s `Expr::Lambda` descent (issue #1685) has
always walked into a lambda's own body as part of the ordinary expression
tree, so any `HirVisitor`-driven CHECK that classifies an expression from
`ctx.locals`/`current_locals()` while visiting inside a lambda body
inherits the same bare-name shadowing hazard the inference frame boundary
exists to avoid — a lambda-own binding (an annotated or unannotated
param, or a name the block's own statements introduce) that shares a name
with a *different-typed* outer local must not be silently classified as
the outer binding's type. The checks this governs: mistyped-field
construction (`E071`, `structs.rs`), conversion domain (`E078`,
`conversions.rs`), or-coalescing (`coalesce.rs`), refined-range
membership (`E117`, `range_refinement.rs`), `contains`-domain (`E152`,
`contains_domain.rs`), and UFCS receiver typing (`ufcs.rs`) — each pushes
a pruned locals frame (`structs::pruned_locals_for_lambda`) on
`enter_lambda`/pops it on `exit_lambda`, keeping only the lambda's own
explicitly `: T`-annotated params (seeded back in under their own names)
and falling back to "unclassifiable" ("Unknown never disagrees") for
every other lambda-own binding, rather than reading through to a
same-named outer binding's type. `option_conditions.rs`'s condition-
position walk (`E116`) shipped this same rule first, unspecced
(#2764/#2768/#2782); this ruling generalizes it to the other six
consumers of the identical hazard class and gives it a normative home.

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
- **`string + T` display-concatenation (RULED 2026-08-01, issue #1911):**
  `+` between a `string` and an `int`/`float`, in either operand order,
  types as `string` — not the same-type unify every other arithmetic use
  of `+` gets. This is not a design choice so much as a description of
  what the runtime has always done: `value_ops::binary_op`'s
  `String`/`Int` and `String`/`Float` `Add` arms already stringify the
  numeric operand unconditionally, with no fault path, so a stricter
  compile-time rule would reject code the interpreter accepts and runs
  correctly — the worst class of diagnostic bug on a checker whose whole
  purpose is to be turned on over real stories. `"score: " + points` and
  ink's chained-concat idiom (`keys + ":" + total`) are exactly this
  shape and are common in real `.brink`/`.ink` source (see
  `tests/tier1-native/for-k-v`'s `sum_and_keys`). The carve-out is
  intentionally narrow, matching the runtime exactly rather than
  generalizing to "string + T": `Add` only (there is no string-numeric
  `Sub`/`Mul`/`Div`/`Mod` at runtime — those keep the same-type unify and
  still report `E066`), and `Int`/`Float` only (`Bool` has no
  `String`/`Bool` `Add` arm at runtime either, so `"x" + true` is still a
  genuine `E066` conflict, not display concatenation). The rule covers
  `+=` too, not just infix `+`: `keys += total` is the same runtime `Add`
  arm as `keys = keys + total` (review finding on this issue's own PR —
  `Stmt::Assignment`/`BlockStmt::Assignment`'s `AssignOp::Add` is a
  separate inference seam from `infer_infix` and needed the identical
  carve-out to avoid rejecting the same legal code under a different
  spelling).

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
- **A post-construction dotted field write (`~ p.y = 3.0` above, or `~ p.y
  += 1.0`) is type-checked against the field's declared type under strict**
  (`E063`, issue #1900) — the RMW-discipline write shown above was, until
  this issue, checked only for missing/extra/mistyped fields at
  *construction* (the bullets above); a later plain assignment to a field
  reached zero type checking. Single-level only (`p.x = v`): a **chained**
  write (`p.a.b = v`, 3+ segments), a **mixed** index/field write
  (`arr[i].x = v`), or an index write whose index chain's own root is a
  struct-field projection (`p.field[i] = v`, `push(p.field[i], v)`, issue
  #2121) is unsupported at any RHS type and is rejected outright with the
  non-suppressible `E074`, never a type-mismatch diagnostic.

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
annotation — since #1877. A **dotted struct-field assignment target**
(`~ p.x = expr`, single-level only — see §6) reaches this same checking
since #1900.

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

## 10. Where the strict pass is exercised — RULED

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

**#1909's free-function half is closed.** A UFCS call that desugars to
a free function (`n.double()` → `double(n)`) now takes that function's
own declared return type, so its result is no longer an `Unknown`
escape, and one baseline row (`ufcs`/`describe_double`) went away. The
call also records the call-graph edge the desugar implies, which is
what makes the target's signature reliably available first. The
*prelude*-verb half (`m.len()`, `tally`'s row) stays open: typing it
means running the intrinsic-typing arms on the desugared argument list,
which would double-report `E149` against `ufcs::check_strict`'s copy —
issue #1540's second symptom, tracked separately.

Two findings land back on this spec rather than on the analyzer. §4's
coercion lattice does not say what `string + T` concatenation does at
all (#1911). And §2's "internal helpers never require an annotation"
promise is not what the corpus experiences: a helper whose body
genuinely constrains nothing (`fn bump(ref n, amount) { n = n +
amount; }`) is an `Unknown` escape — correct under "call-site-driven
inference is forbidden", but not what that sentence leads a writer to
expect. §2 is RULED, so its wording is not re-struck here; the
discrepancy is filed as #1915 for sign-off on the replacement text.

**#1911 is now fixed** (this PR): §4's new `string + T` display-concat
ruling above closes the spec gap this paragraph identifies, and the
`for-k-v` case's two `E066` rows are gone from `tier1_native_strict.rs`'s
`BASELINE` — the sweep's finding count drops from 37 to 35, and `for-k-v`
now produces no strict finding at all, so the case count drops from 7 of
14 to 6 of 14.

**#1910 is now resolved in part** (this PR): `InferPass::infer_lambda`
reads a lambda's own body-derived narrowing back (mono-HM, the same
overlay a top-level `fn`'s own params/return already get) instead of
discarding it and rebuilding the lambda's `Ty::Fn` row from written
annotations alone. Of the 16 `BASELINE` rows attributable to #1910 (the
sweep's `lambda-verbs` case in full, plus `fn-value-bare-name`'s `mixed`
row), 10 are gone: `braced`, `call_through_capture` (return type and both
temps), `chained`, `doubled`, `map_each_scaled`, `positives`, `total`, and
`mixed`. Six remain, none in this PR's scope:

- `scaled`'s parameter `factor` and return type (2 rows): call-site-driven
  inference is forbidden by §2, so `Unknown` is the specified outcome
  here — the same reasoning as `ufcs`'s `bump`/`heal`.
- `ufcs_through_capture`'s return type and temp `f` (2 rows): blocked on
  #1909's own remaining gap (`items.len()`'s UFCS-desugared result still
  types `Unknown`), not on anything #1910 fixes.
- `field_through_capture`'s return type and temp `f` (2 rows): implementing
  #1910 surfaced a separate, pre-existing checker gap (#1924) — a dotted
  field read on a captured struct (`p.x`) types as the whole struct, not
  the field, because no static field-type table exists yet. That gap first
  made these two rows disappear (replaced by one misleading `E063`, since a
  lambda's signature was no longer rebuilt from annotations alone and so
  could surface the mistyped read), then — per a follow-up review fix,
  still within this PR — `infer_lambda`'s overlay was guarded to refuse
  any `body_ty`/`narrowed_params` a walk that hit the mistyped case
  produced, landing these two rows right back at their original, honest
  `E065` shape. Tracked by #1924, unmoved (net) by #1910.
