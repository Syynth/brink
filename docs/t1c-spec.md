# T1c spec — function values as partial application

Status: **draft for ratification** (design round 2026-07-13; core
rulings 2026-07-12, see `docs/decision-log.md`). Companion to
`docs/tier1-roadmap.md` (T1c), `docs/value-model-spec.md` §11
(semantics — ratified), `docs/typed-mode-spec.md` (the `fn(T…): R`
type form this milestone unfreezes with), and `docs/format-v4-rfc.md`
(the wire surface, already reserved). Sections marked **RULED**
transcribe existing decisions; **PROPOSED** sections ratify at this
PR's review.

## 1. The model — RULED

The T1c feature is **partial application over named functions**, not
closures. Ink has no lexical free variables, so there is no
environment to close over; the author-facing name is "function
values / partial application."

A function value is `{fn token, bound-arg row, effect row}`:

- **fn token** — the target's `DefinitionId` (hashes from names, so
  saved tokens survive recompiles that edit bodies).
- **bound-arg row** — a *prefix* of the target's declared params,
  bound at creation. Capture mode comes from the target's own
  signature: a `val` param snapshots the argument value now; a `ref`
  param captures a durable cell (VAR / `#@local`), never a heap
  location.
- **effect row** — present from day one (§7); semantics arrive in T2.

**All `ref` params must be bound at creation**, where the target is
statically named. Thereafter every remaining param is `val`-only, so
dynamic call surfaces are values-only. **No anonymous lambdas in
Tier-1.**

## 2. Creation: `#fn(name, args…)` — RULED (grammar details PROPOSED)

Creation is syntax, joining the `#[…]` / `#{…}` / `Name#{…}` sigil
family, brink-dialect-gated under the T1b superset-grammar rule
(strict-ink rejects at analysis with the standard E051-class
diagnostic; parse never fails).

```ink
=== function heal(ref hp: int, amount: int): int ===
    ~ hp = hp + amount
    ~ return hp

VAR player_hp = 10
~ temp heal_player = #fn(heal, player_hp)   // binds ref hp := cell player_hp
~ temp result = heal_player(5)              // calls heal(ref player_hp, 5)
```

- `name` must resolve to a function definition (knot-function or
  stitch-function) at the creation site — a static reference, not an
  expression. Unknown name is a compile error.
- `args…` bind the declared param row **left to right, prefix only** —
  no skipping, no named binding. `#fn(name)` with zero bound args is
  legal iff the target has no `ref` params.
- A `ref`-position argument must be an lvalue naming a durable cell
  (VAR, `#@local`; `temp` is a compile error — temps die with the
  frame, per value-model §11). Same lvalue discipline as the T1b
  mutator rule.
- Binding *more* args than the target declares is a compile error.
- **Every static obligation lands at this one marked site**:
  name→value, ref binding, effect binding (§7).

**PROPOSED**: `#fn(…)` is legal in expression position only (the T1b
sigil rule — `#` opens a tag in prose position). Diagnostics for the
bullets above allocate from **E079** upward (next free after E078).

## 2a. Creation on the native surface: the bare name — RULED 2026-08-01

`#fn` is the **ink/brink-dialect** spelling and it is not retired. On the
**native** (`.brink`) surface, creation has no sigil at all: a
statically-named function in expression position **is** a fn value.

```brink
fn scene(x) {
  return x + 1;
}

fn used() {
  return map([1, 2, 3], scene);   // a reference — the fn value itself
}

fn called() {
  return scene(1);                // a call — parentheses make it one
}
```

Reference-vs-call is unambiguous because a call keeps its parentheses —
Rust's function-item model. This is why `#fn` is the *wrong* spelling
here rather than merely a redundant one: `#` is already the tag sigil in
native content position (`brink-syntax-native`'s `parser/block.rs`), so
`#fn` would collide with the one meaning `#` has on this surface. The
ruling is a grammar fact, not a taste preference.

Consequences that follow from "bare name, no sigil":

- A **local of the same name wins.** Name resolution reaches a `let`
  binding or parameter before it ever reaches the function definition,
  so `let double = 5; double` is the `int` 5.
- The reference binds **zero** arguments, always. The binding
  (partial-application) form `#fn(f, a)` deliberately has **no** native
  spelling: for a *value* parameter it is now redundant with lambdas
  (`|x| f(a, x)`, since lambda lifting landed), and for a **`ref`**
  parameter it is not redundant but is also not safely respellable —
  lambda capture is by-value always, while `#fn(heal, player_hp)` binds
  a `ref` param to a durable *cell*. Giving that a native spelling needs
  its own ruling and is not blocked by anything today.
- Therefore §2's `ref` obligation survives as an absolute: a target with
  any `ref` parameter can never be referenced by bare name (E080 at the
  reference site). §2's other two creation-site diagnostics do not
  apply — there are no bound args to over-bind (E081), and a bare name
  that resolves to something other than a function definition is simply
  not a fn value at all (E079), it stays a variable read or a knot's
  visit count.
- **Ink is unchanged.** The same bare name in ink is still a knot's
  visit count; only `#fn(f)` creates a fn value there.
- **One spelling per surface, one type, in every position** (issues
  #1876, #1895). A bare-name reference types exactly as the zero-bound
  `#fn` literal does: `§4`'s `fn(T…): R` built from the target's
  signature, carrying the target's own effect row (`FnRow::of_target`,
  effects-spec §5/§6.1a) — it *is* a creation site, so it is harvested
  as one (a call-graph edge plus the Fork A creation atom). That is
  what makes §4's static checking apply to it: passing `f` where an
  `int` is declared is an ordinary `E063`, not an opaque `Unknown`
  deferred to the runtime. Both typing paths key off the same per-file
  frontend flag lowering gates on, so typing and lowering cannot
  disagree about which references are fn values **when the target is
  an actual knot**. The two gates are not otherwise identical: lowering's
  `SymbolInfo::is_function_definition` accepts `SymbolKind::Knot |
  SymbolKind::Stitch` carrying the `"function"` detail, but
  `declared_fn_type`'s `lookup_by_name` restricts the bucket to
  `&[SymbolKind::Knot]` alone — so a top-level stitch promoted to knot
  status (`Knot::symbol_kind`) that carries the same `"function"`
  detail mints a `FnRef` in lowering yet declines to `Ty::Unknown` in
  typing, a genuine one-sided disagreement reachable from a native
  declaration initializer naming that stitch and not yet closed:
  - **Body/expression position** — an argument, an operand, a call
    target — through `HirFile::native` → `Def::native` →
    `BodyCtx::native` (`infer::body`'s `native_fn_value_target`,
    mirroring `lir::lower::expr::lower_path`'s `MakeFnValue` gate).
  - **Declaration-initializer position** (`var f = double;`) through
    `HirFile::native` → `signature::declared_fn_type` (issue #1895),
    mirroring `lir::lower::decls::fold_path_ref`'s `ConstValue::FnRef`
    gate. This makes `Sig::value_ty` a real `Ty::Fn`, so the global
    lands in `infer::collect_globals` and a later `f(3)` type-checks
    instead of misfiring `E065` as an unknown-callee value call.
    Declaration-derived typing has no resolution map, so a bare name
    shadowed by a same-named `VAR`/`CONST`/list item declines to
    `Unknown` rather than guessing the fn interpretation — lowering
    resolves that name to the shadowing global. That guard is a
    project-wide scan (not scoped to what the declaration can actually
    see); issue #1901 asked whether the two could disagree in a
    user-visible way through an unrelated file's same-named global.
    **Review found they did**, for the `ListItem` case specifically: list
    items are indexed under their *qualified* `List.Item` name
    (`manifest.rs`), never the bare item name, so the guard's original
    direct `index.by_name.get(bare_name)` lookup could never see a
    bare-name list item — that arm was dead code for the exact form it
    claimed to guard, and a same-file `flags Palette = double, other` /
    `fn double(...)` / `var alias = double` reference typed `alias` as
    `fn(int): int` regardless, misfiring `E063` against a caller that
    declared an `int` parameter. Fixed by having the guard also run the
    same bare-name suffix scan `lookup_variable` itself uses
    (`resolve::lookup_list_item_bare`), ahead of the knot lookup — see
    `declared_fn_type`'s own doc for the fix and
    `native_bare_name_shadowed_by_a_same_named_list_item_is_not_typed_as_a_fn_value`
    (`crates/brink-compiler/tests/driver.rs`) for the regression test.
    With that fixed, the *cross-file* half of #1901's question still
    closes empirically, for two independent reasons: native
    `VAR`/`CONST`/`LIST` have no publicity mechanism at all
    (`lower_native::decl` hard-codes `visibility: None`), so a same-named
    global in a *different* file can never be legitimately referenced —
    the cross-module privacy gate (`E087`) fails the whole compile before
    the guard's decision could matter; and, independently,
    `resolve::lookup_by_name`'s `lookup_by_name_direct` fast path
    (`resolve.rs:1194`, `if !multiple { return first_match; }`) returns
    the sole candidate of the requested kinds without consulting the
    `ImportScope` at all, so a direct call to the local knot resolves
    correctly regardless of an unrelated file's same-named global (only
    the bare *value* reference hits the kind-priority ahead of that fast
    path and triggers `E087`). See `declared_fn_type`'s own doc and the
    `native_cross_file_global_shadow_of_a_fn_value_reference_fails_to_compile`
    regression test for the full argument.

Respelling ink into native (`brink-respell`) follows the same split: a
zero-bound `#fn(f)` emits as the bare name `f`; the binding form refuses
loudly rather than emitting a lambda with different semantics.

## 2b. A lambda literal decl default — RULED 2026-08-01 (issue #1774)

A native `var`/`const` may also hold a **lambda literal** as its whole
declaration default (`const twice = |x| x * 2`), not just the bare-name
reference §2a ruled. `decls::is_const_foldable_decl_default`'s `Lambda`
arm — which raised `E083` here since #1685 first landed lambdas — is
lifted for exactly this position: `decls::collect_globals`'s
`eval_const_lambda` folds the literal through the same lambda-lifting
machinery (#1709, `lir::lower::lambda::lower_lambda`) every other lambda
uses, just handed an **empty** enclosing frame (no knot/stitch params, no
`~ temp` locals — file scope has none). The synthesized function is a
sibling of the project's knots, addressed via
`lir::ConstValue::FnRef`/`Closure` exactly like a bound `#fn(...)`
literal.

This does **not** relax §1's "no anonymous lambdas in Tier-1" — that
ruling is about the T1c *partial-application* creation form
(`#fn(name, args…)`) never admitting an anonymous body; a lambda literal
is `#1685`'s separate surface, and this section is only about *where*
that already-legal literal is allowed to sit, not a new creation form.

Nested one level in — a lambda literal as a collection element, a
struct field, or a `#fn` bound `val` arg — is **not** covered by this
ruling and stays behind its own `E077`/`E076` diagnostic
(`decls::is_const_foldable_kind`'s `Lambda` arm, deliberately unchanged).

**Why this is safe**, pinning the reasoning rather than leaving it
implicit: the creation-site-capture rule that gates a lambda everywhere
else exists to keep a captured `#@local` cell from leaking outside its
creating flow (the 2026-07-23 "flows-as-actors" direction). A file-scope
lambda has no enclosing frame to capture *from* in the first place, so
that invariant is never at stake — a fact made mechanical, not just
argued, by handing `lower_lambda` an empty `TempMap`/`visible_temps`:
every free name in the body then misses `ctx.temp_slot`, and
`captured_locals`'s own contract treats a miss as "a legitimate
non-local" (a global cell or a knot/function name), never a capture.
Pinned by `brink-ir`'s
`lambda_literal_decl_default_reads_other_globals_without_capturing_them`
(`tests/lir_lowering/lambda_literal_declaration_default.rs`).

**Known follow-ups, not fixed by this ruling:**

- `signature::declared_fn_type` (the typing-side mirror of
  `decls::fold_path_ref`/`eval_const_lambda`, §2a's own note) has no
  `Expr::Lambda` arm — a lambda-valued global types as `Ty::Unknown`
  rather than a real `fn(T…): R`, the same honest "can't determine"
  fallback a shadowed bare-name reference already gets. Not a soundness
  gap (gradual typing's fallback is sound by construction), just less
  precise than the bare-name case.
- A **separate, pre-existing** gap (not introduced or fixed here):
  calling a fn-valued global — bare-name (#1862) or lambda (#1774) —
  from anywhere other than its own declaration does not resolve through
  the production `compile_path` → `brink-db` incremental pipeline
  (`E025`, "unresolved variable reference"), even though the identical
  shape resolves cleanly through the simpler whole-project
  `brink_analyzer::analyze` path `brink-ir`'s own tests use. See the
  #1774 → #2083 (filed separately).
- A narrower, **separate, pre-existing** gap, checked directly against
  this issue's own "does #1764/self-recursion become expressible" ask
  and found still no: a global `const`-bound lambda cannot call *itself*
  recursively by name from inside its own body (`const fact = |n| … n *
  fact(n - 1) …`) — `brink-analyzer` reports `E025` at both occurrences
  of the recursive call. The const's own name, mid-initializer, is not
  yet visible to its own body's resolution (a single-pass ordering
  nuance in the resolver, not the `E083`/capture story this ruling is
  about). Narrower than and adjacent to #2083's territory (that gap is
  about calling a fn-valued global from *outside* its declaration; this
  one is about calling it from *inside* its own declaration) rather than
  a clean independent bug, so not filed separately — pinned by
  `brink-compiler`'s `#[ignore]`d
  `compile_path_native_const_lambda_decl_default_self_recursion_works`.

## 3. Invocation and `bind` — RULED (typing rule details in §4)

Both call forms ship:

- **Direct**: `f(args…)` where `f` is a variable/temp/param holding a
  function value. Supplies the remaining (val-only) params in order.
- **Explicit**: `call(f, args…)` — same semantics, usable where the
  callee is itself an expression.

`bind(f, args…)` is a **stdlib intrinsic** (slice-1 machinery:
VM-native, lowercase, author-shadowable with the E035-class warning)
for val-only currying over *existing* function values — it consumes
the head of `f`'s remaining param row and returns a new function
value. Per the facility doctrine its typing rule and effect row are
declared at introduction: typing = consume the head of the param row
(checker-integrated per §4); effect-transparent — the result copies
`f`'s row.

In **gradual** mode, calling a non-function value, or calling with
wrong arity or a wrong-typed argument, is a **turn-terminating
runtime fault** (value-model §11c posture: no silent garbage).

Cross-flow invocation of a function value that `ref`-binds a
`#@local` is a **turn-terminating fault** in T1c. The ruled
destination is creating-flow identity (#597); late binding is
explicitly not the direction.

## 4. Typing — RULED (this round)

`Ty` gains a structural function type, written `fn(T…): R`
(typed-mode-spec §3 reserved the form).

- `#fn(heal, player_hp)` has type `fn(int): int` — the bound prefix
  is consumed from the target's (inferred or annotated) signature.
  `bind` does the same over values.
- **Under `types = strict`, calls through function values are
  statically checked**: if the callee's type is a known `fn(T…): R`,
  arity/argument mismatches are compile errors; if the callee's type
  is `Unknown`/`Conflicted`, that is an escape error (the existing
  TM-3 escape rule applied to call position). A strict-mode author
  can never reach the §3 runtime fault.
- **Boundary annotations gain the `fn(T…): R` form** so fn-typed
  params can cross host boundaries under strict (`cb: fn(int): int`).
- Gradual mode: everything stays advisory; the runtime fault is the
  backstop. `int → float` coercion applies to call arguments exactly
  as it does to direct calls (§4 lattice).

**PROPOSED**: a function value's type never carries `ref` — refs are
bound away at creation, so `fn(T…): R` param rows are val-only by
construction (the type form needs no mode markers).

## 5. Value semantics: equality, display, restrictions — RULED

- **Equality is structural** (sharing-unobservable invariant): two
  function values are equal iff same fn token and equal bound-arg
  rows (`ref` entries compare by bound cell, `val` entries by value).
  No ordering (`<` etc. is a type error in strict, runtime fault in
  gradual). Not a legal map key (keys stay int/string/bool, ruled).
- **`string(f)` stays total** — the display form is signature-like
  with bound args rendered as defaults:

  ```
  fn heal(ref hp = player_hp, amount)
  ```

  Bound `val` args print their value's display form; bound `ref`
  args print the cell name; unbound params print as bare names.
  Interpolation `{f}` prints the same form (display is universal,
  typed-mode §4). **PROPOSED**: exact rendering above ratifies here —
  it becomes observable surface permanently, so it is deliberately
  boring and stable.

## 6. Persistence, the host boundary, and rehydration — RULED

- **Function values save like every other value.** No serialization
  carve-out: pause/resume must hold (a parked flow can have a
  function value in a cell or on the stack mid-turn), so they appear
  in `SaveState`, journal entries, and speculation snapshots as
  ordinary values.
- Wire form is V4's reserved `VAL_CLOSURE`: `DefinitionId` + env
  entries `{param NameId, val|ref kind, payload}` (`VAL_FN_REF` for
  the zero-bound case). The **named** entries are deliberate
  redundancy: on load/invoke after a recompile, param names and modes
  are validated against the current signature — a mismatch (renamed,
  reordered, re-moded params) is a **defined fault, never silent
  misbinding**. Cross-version loads have no real guarantee; this is
  the best-effort check. A policy lint against long-lived function
  values in saves is possible later; it is not a value-model rule.
- **Host boundary**: function values cross as opaque tokens
  `{DefinitionId, env}`. The host never dereferences the env;
  invocation always re-enters the VM and is journaled. bevy-brink
  gains the host-side callback-invocation surface in this milestone
  (roadmap T1c line item).

## 7. Effect row placeholder — RULED

The effect-row field exists on every function value from creation,
populated with the **pessimal touches-everything row** until the T2
round defines real semantics (rows are conservative-total: unknown =
"may do anything", never omitted). Creation-site binding is the rule
T2 inherits: the row freezes when the value is made, matching the
boundary-freeze symmetry in typed-mode §2. Wire space rides the V4
`EffectRows` reserved section discipline — no format bump needed
when T2 lands.

## 8. Diagnostics — PROPOSED

Allocating from E079 (names indicative; exact split at
implementation review):

- E079 — `#fn` target is not a statically-named function
- E080 — `ref` param not bound at creation (or bound to a non-durable
  lvalue: rvalue / `temp`)
- E081 — `#fn`/`bind` binds more args than the target declares
- Strict call-position mismatches reuse the TM-3 machinery (escape
  errors and typed-mismatch reporting extend to call-through-value
  sites rather than minting parallel codes).
- Runtime faults (gradual dispatch, rehydration mismatch, cross-flow
  ref-`#@local`) are fault variants, not diagnostics.

## 9. Testing (no oracle exists for this surface) — PROPOSED

Per the T1b divergence discipline:

- **Oracle gate**: ratchet `RATCHET_EPISODE_COUNT` byte-identical on
  every slice; strict-ink corpus never sees `#fn`.
- **Tier-1 corpus wing**: `tests/tier1-brink/` grows function-value
  cases with hand-written expected transcripts — creation, both call
  forms, `bind` chains, ref-cell mutation through a stored value,
  save/load mid-turn with a live function value on the stack.
- **Property tests**: display-form stability; structural-equality /
  sharing-unobservable law under randomized bind chains;
  save→load→invoke roundtrip ≡ direct invoke; rename-a-param →
  rehydration faults (never misbinds).
- Grammar fuzzing extends to `#fn` in both dialects.

## 10. Explicitly out of T1c

Anonymous lambdas (no Tier-1 lambdas, ruled); lexical capture of any
kind; creating-flow identity for cross-flow refs (#597 — the fault
ships instead); effect-row semantics (T2, own round); handles (T1d);
projections (T1e); method-call syntax; overloading/defaults on
declarations (binding is the only "default" mechanism).

## 11. Build sequencing — PROPOSED (spine — single reviewed agents, oracle-gated)

1. **T1c-1 grammar + HIR + typing**: `#fn` superset parse + dialect
   gate + E079–E081 + `Ty::Fn` + strict call checking (no
   LIR/codegen — everything still rejects at lowering). This slice
   lands the checker surface first because FG's per-def inference is
   the substrate it plugs into.
2. **T1c-2 LIR + codegen + VM**: `PushFnRef` / `MakeClosure` /
   `CallValue` emission (first live use of the reserved block),
   dispatch + faults, persistence + rehydration validation. Corpus
   wing lands here.
3. **T1c-3 `bind` + `call` stdlib** + display form + host
   callback-invocation surface in bevy-brink.
4. **T1c-4 mechanical tail** (pump wave): corpus growth, book chapter
   ("Function values"), IDE polish (hover shows the bound signature,
   completion after `#fn(`).
