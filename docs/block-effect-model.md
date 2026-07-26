# The unified block / effect / coroutine model

Status: **RULED 2026-07-20 (maintainer-ratified).** Design doc; not yet
implemented. The model governs the native block/effect/coroutine work and
folds into B0.8 (code-body lowering). Deferred design areas are tracked as
stub "needs design" issues (§11). Companion ruling recorded in
`docs/decision-log.md`.

Companions / governing docs this **extends** (does not replace):
`docs/flow-suspension-spec.md` (the FlowFrame model + continuation-splitting
this rides), `docs/effects-spec.md` (the effect rows + wake contract),
`docs/native-surface-charter.md` (the prose/code dialects, §4 "braces are
the universal body delimiter", §5 the prose dialect, §7 RustScript north
star), `docs/hir-admission-contract.md` (the HIR the block substrate must
admit). Interim grammar item this supersedes on landing: #1206 (the G-2
choice-body-vs-interpolation heuristic).

---

## 0. How to read this doc — the build-scope tags

Every component below carries one tag, so this doc doubles as a
migration-scope map:

- **[SUBSTRATE]** — already exists and is reused **as-is**. No change.
- **[REFACTOR]** — already exists but must be **reshaped** to fit. The
  behavior is mostly preserved; the representation moves.
- **[NEW]** — a genuinely new addition with no current analogue.

The consolidated tag table is §10. The thesis first, then the model, then
the type rules, then the lowering, then the scope map.

---

## 1. Thesis

**Meaning comes from analysis, not from the parser's guesses.** The parser
stays syntax-directed and never disambiguates by heuristic — it emits
uniform structure; wherever meaning is ambiguous from syntax alone (the
*same* surface can mean different things), the **type / analysis layer**
resolves it. `Block` + `tail` + the effect row are the machinery that lets
it.

This is the insight that held up. (An earlier draft led with "a block is
an expression" and leaned on a structural *unification* of the
per-construct nodes; the scoping pass showed that unification is low-value —
the structural distinctions are real and merely *relocate*, see §3a. The
durable win was never structural; it was moving semantic decisions off the
parser.)

> A body is a uniform `Block { stmts, tail }`; a braced construct's
> *meaning* — interpolation vs. body, value vs. control transfer — **falls
> out of the tail's type and the block's inferred effect row**, decided by
> the checker, not committed by the parser. A **flow is a coroutine**; a
> function is the degenerate flow that never suspends; value-return is
> orthogonal to suspension.

The sharp cases:
- **G-2 dissolves.** The parser stops guessing interpolation-vs-body — it
  emits a braced block; `{ gold }` vs `{ -> shop }` differ only in their
  **tail** (value ⇒ stringify-and-emit; diverge ⇒ control flow), read by
  the checker, not a parse-time lookahead.
- **The "constructs embedded in content" grammar holes dissolve.** The
  parser emits uniform content; an embedded construct's meaning is derived,
  so there is no per-position special-casing to forget.
- **Effects are inferred, never parsed or annotated** — "what a block does"
  is a type-layer fact (the one canonical effect row, `effects-spec.md`
  §14).

**The precise rule**, held honestly: the parser stays structural and
**never guesses**; where syntax is *genuinely distinct* (`{~a|b}` vs
`{cond: …}` vs `{? *…}`) it still commits to the construct — no ambiguity
to defer. Only the **ambiguous overlaps** (same surface, different meaning)
move to the type/analysis layer. The old ink parser broke this separation
(semantics smuggled into lookahead); this restores it.

The prose and code dialects are two **skins** over the one `Block`
substrate — which *already exists* (§2): prose is content-lines-by-default
with code interlaced, code the reverse. Neither owns emission, control
transfer, or suspension.

---

## 2. The block substrate

**[REFACTOR — corrected by the 2026-07-20 scoping pass.]** One shared body
IR **already exists**: `Block`/`Stmt` in `brink-ir` (`hir/types.rs`,
documented as "the universal body type"), and both frontends already target
it. The per-construct types (`Conditional`, `Sequence`, `ChoiceSet`/`Choice`,
`ThreadStart`) are `Stmt` *variants inside* that shared `Block`, not separate
body structures. So this slice does **not** introduce a shared type — it
**evolves the existing `Block` in place**: add a `tail` field, add an effect
signature, and carry the structural kind (`SequenceType`, `CondKind`, choice
flags, `ChoiceSet.continuation`) as **side-data on `Block`**.

Honest scope (the scoping correction): the structural control-flow kinds do
**not** dissolve. They survive to LIR `ContainerKind` and drive codegen, so
they must persist as data, not be erased into effects (§3a). **The
unification is on the surface / tail / effect axes; the structural zoo is a
separate axis, reduced only later** (north star #1213). No crate relocation
— moving the ink-HIR into `brink-syntax` would invert a dependency edge into
a cycle the codebase already rejected (`lower_native/mod.rs` judgment call
#1); instead the ink-shaped `Stmt` variants become a `brink-ir`-private
module that stops being the cross-frontend interface. **Oracle firewall:**
codegen has zero `hir::` references, so freezing LIR `ContainerKind` output
and proving byte-identity at `RATCHET_EPISODE_COUNT` after every slice keeps the migration
safe. B0.6/B0.7 lowering is reshaped; their **behavioral tests survive**.

**[SUBSTRATE]** The **tail** taxonomy already has its pieces: a tail is
`Value(expr)` | `Diverge(terminator)` | `Unit`. `Diverge` is the
`!`-typed terminator — divert / `->->` / `return` / `END` / `DONE`. This
is exactly Rust's never-type discipline (dead code after divergence,
`!` coerces anywhere) and `ReturnKind` (B0.2) already distinguishes the
return flavors.

---

## 3. Effects — the behavior signature

**[REFACTOR — reconciled with `effects-spec.md` §14, 2026-07-21.]** A
block's meaning is its inferred effect signature, and that signature is the
**one canonical effect row** ruled in `effects-spec.md` — *not* a new
parallel lattice. An earlier draft of this section listed
`Emit / Transfer / Suspend / World / Impure` as if they were a fresh effect
system; that over-unified and conflicted with the shipped row. The correct
mapping onto the ruled row:

- **Emit** → the row's existing `emits` dimension. **[REFACTOR]** Prose
  emission exists but making "does this block emit" an *inferred* row
  dimension (so content interlaced in a code block is visible to the
  checker) is the wiring.
- **Suspend(rung)** → **folds into the row** as the new `suspend(rung)`
  dimension (`effects-spec.md` §14.2). Suspension "color" is not a separate
  system — it *is* this dimension; the no-call-up-the-ladder rule (§4, §7)
  is an inferred check over it.
- **World** → the row's existing `calls` (external bindings); the
  read/command granularity stays host-side (`CapabilityEffects`), by the
  deliberate compiler↔bevy independence. **[SUBSTRATE]**.
- **Transfer** → **NOT a row effect.** General control-transfer (a plain
  divert) is *structural* — the block's `tail` (§2), enforced by the
  no-lateral-divert rule (§7.1). Only the terminal `-> END/DONE` case is a
  candidate row dimension (`terminates`, provisional — `effects-spec.md`
  §14.2), for structured-concurrency lifetime.
- **Impure(sequence)** → **OUT of the row** (`effects-spec.md` §14.3 / §10
  NS-A6 posture: the visit cursor is an unmodeled read, flow-local, never
  in a fusion callback). Separately and unchanged: the structural
  `SequenceType` (which selection discipline) survives to LIR
  `ContainerKind` and drives codegen, so it **stays on the `Sequence`
  `Stmt` variant where it already lives** (not moved onto `Block`) — a
  *structural-kind* fact independent of effects. Whether the variants ever
  collapse is the deprioritized #1213 question (§3a).
- **Pure** = the empty *effect axis*; `reads` are the **dependency axis**
  (the wake-map's set, a coeffect), and do not make a block impure
  (`effects-spec.md` §14.4). Fusion uses the reads-OK (weak, E105)
  purity predicate.

Interpolation is not a distinct construct: it is a block whose tail is a
`Value` and whose position is content — the checker stringifies-and-emits
the value. That is the G-2 dissolution stated in effect terms.

---

## 3a. What this unifies — and what it deliberately does not

Stated honestly, because the scoping pass showed the original framing
("constructs collapse to configurations of one node") oversold it:

- **Genuinely unified — the real wins:** the **surface** (a block is a block
  everywhere; the "embedded in content" grammar class dissolves), the
  **tail** taxonomy (value/diverge/unit — dissolves G-2), the **effect
  signature** (a cross-cutting axis carrying content-as-effect, the coloring
  rules, the type rules), and **value-returning flows / coroutines**. These
  land regardless of the structural kinds and are what the model is *for*.
- **Deliberately not unified — the structural control-flow kinds** (Sequence
  vs Conditional vs Choice). These are genuinely *different machines* — a
  visit-counted cursor, a predicate branch, an interactive-and-convergent
  choice — and their distinctions survive to LIR `ContainerKind` and drive
  codegen. They **stay as the existing `Stmt` variants inside `Block`**;
  they are not moved onto `Block`.

**On #1213 (minimal orthogonal core) — a guiding value, NOT a committed
slice.** The honest cost/benefit (assessed 2026-07-21): collapsing the
structural variants (e.g. `Conditional`+`Sequence` → `Select { branches,
discipline }`; `Choice` resists — richer, interactive+convergent) does
**not** erase the distinction — the selection discipline survives to
codegen, so the dispatch *relocates* (from `Stmt`-variant to a `.discipline`
field), it doesn't vanish. The real payoff is narrow: **deduplicating the
triplicated "walk the branches" traversal** + the orthogonal-basis
aesthetic. That's a tidiness/refactoring win, not a capability one, against
oracle-guarded surgery. So: **minimal-orthogonal-core stays a guiding value**
(bias new design toward the small basis), but the structural collapse is a
"do it only if the triplicated-traversal pain is actually felt, or a new
construct clearly benefits" call — not a planned migration. #1213 is
reframed accordingly. **There is no `Block`-structural-kind slice (the old
"S2") in the migration** — see §11.

---

## 4. The suspension ladder = the color

**[REFACTOR]** `flow-suspension-spec.md` §4 already draws the color
boundary — functions synchronous, tunnels await. This model **generalizes**
that one boundary into a ladder, ordered by *who resumes*:

| Rung | Resumer | Persistence | Value-context legality |
|---|---|---|---|
| **await** | the engine (`resolve_external`) | ephemeral, or durable when persistently parked (§10.3) | operand-position via ANF (§8) |
| **choice** | the player (`choose`) | **durable** (save point) | operand-position via ANF (§8) |
| **turn / DONE** | the driver (`continue`) | **durable** (save point) | n/a (statement boundary) |

A block's **color is the outermost rung it can reach.** A pure fn reaches
nothing (or only await, if async). A flow reaches choice and turn.

**[NEW]** Two consequences become type rules (§7):

- **Call direction.** You may call *down* the ladder freely (a flow calls
  a fn); calling *up* (a fn calls a flow) is the up-coloring violation.
- **Persistence.** A **durable** pause (choice, turn) is a save point and
  must occur with an empty operand stack (a structural boundary). An
  **ephemeral** pause (await, resolved within the turn) may occur
  mid-expression and is never serialized. This is *why* no stackful trace
  is ever stored — see §8.

**[SUBSTRATE]** The three resume mechanisms already exist and are genuinely
separate: await = an `External` frame swapped on `resolve_external`;
choice = a full `Thread` clone swapped on `select_choice`; turn = a
`StoryStatus` flag flipped `Done → Active`. The ladder *names* what the
runtime already does.

---

## 5. Value-returning flows — RULED (sitting)

**[NEW]** A flow may declare a return type and return a value. This is the
construct missing from the middle of the fn/flow spectrum: an emitting,
player-interactive, suspendable computation that *also* yields a final
value to a caller who awaits it — a coroutine. (The bartering-minigame
that returns the final price, today faked with globals.)

- **[SUBSTRATE]** The return **mechanism** needs nothing new for the
  in-memory happy path: return works by leaving the value on the one
  Flow-global `value_stack` when the callee frame pops — untouched by frame
  push/pop, so it works for *any* frame type at a statement boundary, not
  just `Function` frames. A flow frame reuses it directly.
- **[REFACTOR]** Making value-return an **explicit, typed** concept rather
  than "incidentally left on the shared stack," and allowing a *flow* (not
  only a fn) to carry a return type through the checker and the frame
  model.
- **[NEW]** The **return-type declaration is the toggle** between a flow's
  two lives: declare a return type → **coroutine** (must yield a value, may
  not laterally divert away); declare none → **state** (an FSM node, may
  divert away, no caller awaits a value).

**Durable coroutine state** rides existing machinery: a coroutine parked at
a choice spills its in-flight locals into the **[SUBSTRATE]** name-keyed
`SuspendedFlow.frame` (crossing locals, no stack, no offsets). Extending
the FS-3 spill/restore to carry a value-returning coroutine's crossing
locals is **[NEW]** runtime work (FS-3r is unbuilt regardless).

FSMs fall out, they are not a feature: **flows are states and transitions**
(they own Transfer); **fns are the pure transition logic** — a fn holds
`match state { A => -> a, B => -> b }` where each `-> x` is a divert-*target
value*, and **returns** the chosen target; the flow **follows** it. The fn
computes *which*; it never *performs* the transfer. (Correction banked
during the sitting: recording a divert-target is pure data and fn-safe;
*following* a divert is the flow-colored effect. Up-coloring a fn to follow
a divert is rejected.)

---

## 6. Divert targets as values — the record/follow split

**[SUBSTRATE]** ink already has divert-*target* values (`VAR dest = -> x`,
later `-> dest`); the runtime carries them (`DivertTarget`, computed-divert
opcodes). This is the "record" half — pure data, legal in a fn.

**[NEW, type rule]** *Following* a target (performing the transfer) is the
Transfer effect and is flow-colored. The distinction is what keeps FSM
transition-logic (computing targets, §5) pure while the transition-act
stays in flows.

---

## 7. Type rules — NEW

All checker-level; the runtime needs no new field to enforce them (it
provides the structural call-vs-divert distinction — `return_address`
`Some`/`None`, frame-push vs in-place `goto` — to build on).

1. **No lateral divert from a value-position flow.** A flow that declares a
   return type may not exit by a lateral divert — that would abandon the
   caller awaiting the value. This is **deliberately stricter than Rust's
   `!`-coercion**: Rust's divergence sources return or unwind (both account
   for the caller); brink's divert is a *lateral* transfer to another live
   flow, which does not. So a lateral exit from a value-flow is an error,
   not a coercion.
2. **No calling up the ladder.** A block may not call a construct whose
   color (outermost suspension rung) exceeds its own. A fn cannot call a
   flow; a fn cannot await unless async-colored (per `flow-suspension-spec`
   §4, functions are synchronous).
3. **Any suspension in operand position is ANF-hoisted** (§8) — uniformly,
   whether await, choice, or a coroutine call. The surface is liberal; the
   checker only requires the hoist be possible (it always is) and the
   lowering moves the suspension to a statement boundary.
4. **No up-coloring a fn to follow a divert** (a corollary of 1–2 stated at
   the fn boundary): a fn may hold and return divert targets but may not
   perform a transfer.

*Deferred (not in v1):* the **entry-mode dual** — that a return type is
meaningful only for *call-entry*, not *divert-entry* (a flow entered by a
lateral divert has no caller awaiting a value). Rule 1 is the local
approximation; the caller-side whole-program check is future work.

---

## 8. ANF lowering — the syntax/IR reconciliation — RULED (sitting)

**Surface liberal, IR sane.** Mid-expression suspension (a choice/value
block inside an expression, `let x = ({ choice }) * 2`) is allowed at the
surface. The IR normalizes it to **A-normal form**, hoisting each
suspending sub-expression into a preceding binding so the suspension lands
at a statement boundary:

```
let x = ({ choice block }) * 2      // surface
--- lowers to ---
let __t = { choice block }          // suspension HERE — statement boundary, operand stack empty
x = __t * 2                         // pure arithmetic
```

Operands to the left of a suspension become named temps that cross the
yield **by name**, never as stack operands:

```
let __a = f(x); let __t = { choice }; __a + __t
```

This is the crux: **ANF converts "operands live on the eval stack across a
yield" into "named locals live across a yield"** — exactly the
name-keyed representation the save format already carries. At every actual
suspension the operand stack is empty; all live state is named. **No
stackful trace is ever stored**, honoring `flow-suspension-spec` §2 ("no
instruction offsets, ever") and the maintainer's constraint.

- **[SUBSTRATE]** The back-half already exists: `flow-suspension-spec`
  §11.1 **continuation-splitting** — everything after a suspension becomes
  a synthesized, invisible (§11.2) continuation container entered by an
  ordinary divert; spill-on-park (§5) moves crossing locals into the frame.
- **[NEW]** The **ANF hoisting pass** itself — the front-half that
  normalizes a mid-expression suspension to statement position before the
  existing splitting runs. Extends FS-2's per-site liveness/splitting to
  run on ANF-normalized IR.

**Scope of the liberalization — RULED (sitting): uniform.** Any expression
whose effect signature includes a suspension or a value-yield — choice
blocks, value blocks, coroutine calls, **and `await`** — gets the liberal
surface and is ANF-lowered identically. The earlier "choice/value only,
await stays statement-only" carve-out is **dropped**: carving one construct
out of a uniform shape is exactly the exception authors must forever
remember, and the taste argument behind it doesn't earn that cost. The one
real asymmetry — an `await` is an *invisible* engine pause where a choice
is a *visible* player act, so a mid-expression await hides a turn-split with
no cue — is handled where visibility belongs: the **editor** surfaces
suspension points (semantic tokens / the live renderer — the NS-T
workstream), not the grammar. Uniformity in the language; visibility in the
tooling. (This liberalizes `flow-suspension-spec` §3's statement-only
*surface* rule for await; the §3 machinery is untouched, and functions stay
synchronous per §4 — coloring is unaffected.)

---

## 9. Reconciliation with the governing specs

- **`flow-suspension-spec` §4 (fn/tunnel color) — preserved & generalized.**
  The ladder (§4 here) is a superset; the fn/tunnel boundary is two of its
  rungs. Nothing in §4 there is contradicted.
- **`flow-suspension-spec` §3 (`await` statement-only) — a *surface* rule,
  scoped to await.** This model liberalizes *choice/value* blocks (via
  ANF), not await, unless overridden. §3's machinery is untouched; only its
  surface reach is discussed.
- **`flow-suspension-spec` §2/§5/§11 (FlowFrame, spill, splitting) —
  reused wholesale** as the coroutine substrate.
- **`effects-spec` — extended, not forked.** New rows (Emit, Transfer,
  Suspend, sequence-Impure) join the existing lattice.
- **#1206 (G-2 heuristic) — superseded on landing.** The parse-time
  choice-body/interpolation heuristic is replaced by the type-time
  block-expression + effect decision. Until this model builds, #1206's
  interim ruling stands.

---

## 10. Build-scope map (the consolidated tag table)

| # | Component | Tag | Notes |
|---|---|---|---|
| a | FlowFrame suspended-state representation | SUBSTRATE | `flow-suspension-spec` §2 |
| b | Continuation-splitting + invisible continuation containers | SUBSTRATE | §11.1/§11.2 |
| c | Spill-on-park, name-keyed frame record, drift/rehydration | SUBSTRATE | §5/§7/§10.3 |
| d | Flow-global `value_stack` + statement-boundary value handoff | SUBSTRATE | the return mechanism, any frame type |
| e | Call-vs-divert structural distinction (`return_address`, goto) | SUBSTRATE | the basis for type rules 1/4 |
| f | Output isolation (capture-depth counter) | SUBSTRATE | reused for wakeCheck (§11.3) & coroutine isolation |
| g | `!`-tail terminators + `ReturnKind` | SUBSTRATE | B0.2 |
| h | Divert-target *values* + computed-divert opcodes | SUBSTRATE | the "record" half (§6) |
| i | FrameShapes section + per-site liveness analysis | SUBSTRATE | FS-3c, already landed |
| j | Evolve the existing shared `Block`: add `tail` (S1, **done**). Structural kinds **stay on the `Stmt` variants** (not moved onto `Block` — that's #1213, deprioritized §3a); effects are **per-definition** (`effects-spec` §4), not a `Block` field. | REFACTOR | no crate move; oracle-firewall at LIR `ContainerKind`; the only real `Block` change is `tail` |
| k | Brace disambiguation: parse-time heuristic → type-time effect decision | REFACTOR | dissolves G-2; parser stops guessing |
| l | Wire the canonical effect row to native HIR; add `suspend(rung)` + provisional `terminates` dimensions; build §6.1 row-poly | REFACTOR | extend the ruled `effects-spec.md` row (§14), do not fork; Transfer=tail, seq-impurity out |
| m | Value-return made explicit & typed; flows may declare return types | REFACTOR | the mechanism (d) is substrate |
| n | Suspension: generalize await-only → the await/choice/turn ladder | REFACTOR | choice/turn parks under the FlowFrame umbrella |
| o | Value-returning flows / coroutine-vs-state + return-type toggle | NEW | §5 |
| p | Type rule: no lateral divert from a value-flow | NEW | §7.1 |
| q | Type rule: no calling up the ladder (generalized coloring) | NEW | §7.2 |
| r | Sequence impurity effect row (additive); structural `SequenceType` **persists** as side-data | NEW+REFACTOR | effect additive; structural kind NOT erased (§3a, #1213) |
| s | Emit-effect inference for code-dialect blocks | NEW | prose emits today, untracked |
| t | ANF hoisting pass (front-half of continuation-splitting) | NEW | §8 |
| u | Mid-expression suspension surface + its ANF obligation | NEW | RULED (sitting); choice/value blocks |
| v | FS-3r spill/restore extended to coroutine crossing-locals | NEW | FS-3r unbuilt regardless |

Rough reading: the *runtime plumbing* is overwhelmingly **SUBSTRATE** — the
coroutine machine already exists (`flow-suspension-spec` designed it). The
*HIR/checker* is where the **REFACTOR** and **NEW** concentrate — block
unification, the effect rows, the ladder generalization, and the value-flow
type rules.

---

## 11. Deferred design & open items

**Deferred design areas — tracked as stub "needs design" issues, picked up
when ready (not blocking this model):**

- **Flow concurrency / structured spawning** → **#1210.** Language-level
  `spawn` + structured-concurrency scope (goroutine-shaped but scoped),
  with a detached form for ambient flows. The scheduling substrate already
  exists (park/wake, batch drivers); the gap is the spawn surface + scoping.
  Composes with value-returning flows (join yields the value).
- **The effect system** → **#1211**, now *largely settled* (see
  `effects-spec.md` §14 + the 2026-07-21 decision-log entry): one canonical
  effect row (checking-discipline, not handlers; lattice + shallow
  row-polymorphism; `suspend(rung)`/provisional `terminates` dimensions;
  `reads` = dependency axis). The remaining work is wiring the ruled row to
  native HIR + building §6.1 — not a fresh calculus. (#1211's original
  "row-accretion will roost" framing is superseded: the row was already
  ruled, not accreted.)
- **Post-landing runtime restructuring** → **#1212.** Explicitly *after*
  this model builds — the §10 REFACTOR rows are its scope surface.
- **Minimal orthogonal core** → **#1213**, reframed to a **guiding value,
  not a committed slice** (§3a): collapsing the structural variants
  relocates the discipline dispatch rather than erasing it, so it's a
  tidiness win against oracle-guarded surgery — do it only if the
  triplicated-traversal pain is felt or a new construct clearly benefits.
  The bias-toward-a-small-basis stays; the specific collapse is not planned.

**Open item still in this doc (v2, not blocking v1):**

- **Entry-mode dual** (§7 deferred) — the caller-side of the value contract
  (a return type is meaningful only for call-entry, not divert-entry). Rule
  §7.1 is the local approximation; the whole-program check is v2.

**Migration chain (as of 2026-07-21):**

- **S1 — add `tail` to `Block`** — DONE (landed, oracle byte-identical).
- **~~S2 — structural-kind side-data on `Block`~~ — DROPPED.** The
  structural kinds already live on the `Stmt` variants (readable, correct,
  surviving to LIR); moving them onto `Block` is the deprioritized #1213,
  not a migration step. No S2.
- **S3 — re-point consumers** to read `tail` (and later effects) off
  `Block`, keeping the `Stmt`-variant structural handling as-is.
- **S4 — wire the ruled effect row** (`effects-spec.md` §14) to
  native-lowered HIR + build §6.1 shallow row-polymorphism.
- **S5 — native code bodies emit `Block`** (B0.8).

All off the "author a scene" critical path; each oracle-guarded
byte-identical.

---

## 12. What this is not

Not a semantics change to the runtime — the VM still emits, diverts, pushes
tunnel frames, defers choice thunks, and parks via FlowFrames exactly as
designed. This is a *representation* unification (block substrate + effect
signature) plus **one** new capability (value-returning flows) and **one**
new surface affordance (mid-expression suspension, ANF-lowered). Correctness
above all: the coroutine machine is reused, not reinvented; the oracle bar
(`flow-suspension-spec` §10.4: byte-identical at `RATCHET_EPISODE_COUNT`, vanilla-unreachable
opcodes) is inherited unchanged.
