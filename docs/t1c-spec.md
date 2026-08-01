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
- **One spelling per surface, one type** (issue #1876). The reference
  types exactly as the zero-bound `#fn` literal does: `§4`'s
  `fn(T…): R` built from the target's signature, carrying the target's
  own effect row (`FnRow::of_target`, effects-spec §5/§6.1a) — it *is*
  a creation site, so it is harvested as one (a call-graph edge plus
  the Fork A creation atom). That is what makes §4's static checking
  apply to it: passing `f` where an `int` is declared is an ordinary
  `E063`, not an opaque `Unknown` deferred to the runtime. Inference
  keys this off the per-file frontend flag (`HirFile::native` →
  `Def::native` → `BodyCtx::native`), the same flag lowering gates
  `MakeFnValue` on, so typing and lowering cannot disagree about which
  references are fn values.

Respelling ink into native (`brink-respell`) follows the same split: a
zero-bound `#fn(f)` emits as the bare name `f`; the binding form refuses
loudly rather than emitting a lambda with different semantics.

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
