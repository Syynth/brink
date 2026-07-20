# The unified block / effect / coroutine model

Status: **DRAFT — PROPOSED 2026-07-20, awaiting maintainer ratification.**
Three rulings *were* called during the design sitting that produced this
doc (marked **RULED (sitting)** below); the rest is proposed and must be
reviewed before it governs any build. Nothing here is implemented.

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

One substrate, two orthogonal descriptions:

> **A block is an expression.** Its *value* is its tail; its *behavior* is
> its inferred **effect signature**. What a braced construct "is"
> (interpolation, choice body, conditional arm, fn body, flow body,
> lambda) is not a syntactic category — it is a block plus the effects its
> body performs, decided by the type checker, not the parser.
>
> **A flow is a coroutine.** A function is the degenerate flow that never
> suspends. Value-return is orthogonal to suspension: any block may yield a
> value; any flow-colored block may also emit, transfer control, and
> suspend.

The prose and code dialects are two **skins** over this one substrate.
Prose is a block whose statements are content-lines by default with code
interlaced; code is the reverse. Neither owns emission, control transfer,
or suspension — those are effects any block can carry.

This dissolves the recurring "constructs embedded in content" grammar
class (a block is a block everywhere) and, specifically, dissolves G-2:
`{ gold }` and `{ -> shop }` stop being two syntactic things the parser
must disambiguate and become one thing — a block-expression — whose
meaning falls out of its tail's effect (value ⇒ emit-stringified;
transfer ⇒ control flow).

---

## 2. The block substrate

**[REFACTOR]** Today the HIR has separate node families — `Conditional`,
`Sequence`, `ChoiceSet`/`Choice`, `ThreadStart`, fn bodies, lambda bodies
— each re-encoding "what happens here." They collapse onto a single
`Block { stmts, tail }` carrying a **mode/effect descriptor**. The
constructs become *configurations* of one node, not distinct kinds. B0.6
declaration lowering and B0.7 body lowering (which produce the current
per-construct nodes) are the code reshaped by this; their **behavioral
tests survive** the reshape (they pin observable lowering, not node
identity).

**[SUBSTRATE]** The **tail** taxonomy already has its pieces: a tail is
`Value(expr)` | `Diverge(terminator)` | `Unit`. `Diverge` is the
`!`-typed terminator — divert / `->->` / `return` / `END` / `DONE`. This
is exactly Rust's never-type discipline (dead code after divergence,
`!` coerces anywhere) and `ReturnKind` (B0.2) already distinguishes the
return flavors.

---

## 3. Effects — the behavior signature

**[REFACTOR]** A block's meaning is its inferred effect signature, layered
on the **existing** effect machinery (`effects-spec.md`'s rows + the
bevy-brink binding effects: pure / command / world-query, plus the
fn-color axis). We do **not** spawn a parallel effect system; we add rows
to the one that exists. The rows this model needs:

- **Emit** — appends to the transcript. **[REFACTOR]** Prose emission
  exists but is not effect-*tracked*; making "does this block emit" an
  inferred effect (so a content line interlaced in a *code* block is
  visible to the checker) is the reshape.
- **Transfer** — control leaves via a divert/return/end (the `!` tail).
  **[NEW]** as a tracked effect, though the underlying control flow is
  substrate.
- **Suspend(rung)** — parks for resumption; see §4. **[REFACTOR]** await
  suspension is designed (`flow-suspension-spec` §3); choice/turn parks
  become the same effect at other rungs.
- **Impure(sequence)** — **RULED (sitting):** the ink sequence /
  alternative family (cycles, shuffles, once-only, `{a|b|c}`) is **an
  effect** — it violates purity (stateful selection advances a hidden
  cursor) but nothing else. It is not a separate block universe; it is an
  ordinary block whose selection carries an impurity effect. **[REFACTOR]**
  of how sequences are modeled (they exist as `Sequence` HIR today; they
  become "a block with the sequence-impurity effect").
- **World** — the existing pure/command/world-query binding effects.
  **[SUBSTRATE]**, reused unchanged.
- **Pure** — the absence of all of the above. **[SUBSTRATE]** concept.

Interpolation is not a distinct construct: it is a block whose tail is a
`Value` and whose position is content — the checker stringifies-and-emits
the value. That is the G-2 dissolution stated in effect terms.

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
| j | Per-construct HIR node families → one `Block { stmts, tail }` + mode | REFACTOR | reshapes B0.6/B0.7 lowering; behavioral tests survive |
| k | Brace disambiguation: parse-time heuristic → type-time effect decision | REFACTOR | dissolves G-2; parser stops guessing |
| l | Effect system: add Emit/Transfer/Suspend/sequence-Impure rows | REFACTOR | extend, do not fork |
| m | Value-return made explicit & typed; flows may declare return types | REFACTOR | the mechanism (d) is substrate |
| n | Suspension: generalize await-only → the await/choice/turn ladder | REFACTOR | choice/turn parks under the FlowFrame umbrella |
| o | Value-returning flows / coroutine-vs-state + return-type toggle | NEW | §5 |
| p | Type rule: no lateral divert from a value-flow | NEW | §7.1 |
| q | Type rule: no calling up the ladder (generalized coloring) | NEW | §7.2 |
| r | Sequence-as-effect (impurity) modeling | NEW | RULED (sitting) |
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

## 11. Open questions (must resolve before this governs a build)

1. **The effect-lattice join** — the composition of Emit / Transfer /
   Suspend(rung) / sequence-Impure with the existing pure/command/world-query
   + fn-color rows is asserted (§3) but not worked out as a lattice. The
   maintainer has flagged (correctly) that the current row-accretion is
   **not a properly designed effect system** and will "come home to roost."
   The failure mode is precisely accretion without a core calculus: before
   it roosts, a dedicated effects sitting should pick a small core (what an
   effect *is*; how effects compose; the row/lattice discipline) and
   *derive* the rows from it, informed by prior art (row-polymorphic effects
   à la Koka, algebraic effect handlers, capability/coeffect systems) rather
   than invented cold. Not v1; flagged now so it is not forgotten. Most of
   what brink calls "effects" so far (purity, world-access, fn-color,
   suspension) maps onto known systems — this is not uncharted, just
   under-designed.
2. **Entry-mode dual** (§7 deferred) — the caller-side of the value
   contract. Explicitly out of v1.
3. **Sequencing** — B0.7 (prose bodies) is parked at its review gate; B0.8
   (code bodies) is where this model lands. Proposed: land B0.7 (behavior
   pinned), hold before B0.8, run the design-ratification of this doc, fold
   the ratified model into B0.8's scope. Decide.
4. **Post-landing runtime restructuring** (flagged by the maintainer;
   explicitly *can wait*) — once the block/effect/coroutine model lands, the
   runtime likely has restructuring opportunities the model exposes (e.g.
   `CallFrameType` variants collapsing once fn = flow-with-no-suspension;
   return handling unifying once value-return is uniform; the frame model
   simplifying around FlowFrame). The §10 REFACTOR rows are the surface such
   a restructuring would touch. Reserve a sitting for it *after* the model
   builds — precedent is `docs/runtime-restructuring-spec.md` (the completed
   9-step effort). Not now.

---

## 12. What this is not

Not a semantics change to the runtime — the VM still emits, diverts, pushes
tunnel frames, defers choice thunks, and parks via FlowFrames exactly as
designed. This is a *representation* unification (block substrate + effect
signature) plus **one** new capability (value-returning flows) and **one**
new surface affordance (mid-expression suspension, ANF-lowered). Correctness
above all: the coroutine machine is reused, not reinvented; the oracle bar
(`flow-suspension-spec` §10.4: byte-identical at 5,577, vanilla-unreachable
opcodes) is inherited unchanged.
