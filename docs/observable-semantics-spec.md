# Observable runtime semantics — the equivalence definition

Status: **RULED 2026-09-01** (definition, invariants, guarantee ladder;
`docs/decision-log.md` "Observable runtime semantics: the host-facing
trace" and its two companions of the same date). Consumers: auto-fix's
notion of a *Safe* fix (its own spec, forthcoming), the optimizer (not
yet designed — this definition is a prerequisite, not a consequence),
`brink fmt`, `brink-respell`, and incremental lowering. Tracked at
#3371 (the oracle) and #3370 (the program generator).

## 1. Why this exists

Several features need to claim "this transformation does not change
the story": an auto-fix that deletes a bare `~`, a formatter, the
respeller, an optimizer pass, an incremental re-lowering. Without one
shared definition each of them would invent its own bar — and the
obvious candidate, *byte-identical compiled output*, is wrong twice
over: it rejects harmless transformations (reordering a `LIST`
declaration renumbers items; an optimizer cannot exist under it) and
it is not what an author means. What an author means is: **a host
running the story cannot tell the difference.** This document says
exactly what a host can tell.

## 2. The definition — RULED

Fix a **run**: a start point, an RNG seed, a choice sequence, and a
fixed set of external-function results. Two programs `P` and `Q` are
**observably equivalent** iff every run produces the same **trace**,
where the trace is:

1. **Output steps, in order.** Every `Step::Line` (its `text`, `tags`,
   and `element.data`), every `Step::Choices` (each choice's text and
   tags, **in order** — hosts pick by index, so order is observable),
   and the terminal kind reached (`Done` / `End` / `Suspended`).
2. **External calls, in order, with arguments.** The host sees these
   and their results feed back in; both direction and order are
   observable.
3. **Host-readable state at every turn boundary.** The values of
   global `VAR`s and `LIST`s. Hosts read them (`getVar`, engine
   bindings that watch variables, saves that capture them), so a
   transformation that changes a global the story itself never reads
   is still observable. RULED **in**.
4. **Host-invoked function results** (`callFunction` and the flow
   entry points that evaluate a function from engine code), together
   with any output such a call emits.

Everything else is explicitly **unobservable**:

- bytecode layout, container numbering, instruction counts, step
  counts, memory, timing;
- compile diagnostics and **runtime warnings** — the diagnostics
  channel is a development surface, not story semantics;
- temps, the value stack, the call stack, and visit counts *as
  internals* (visit counts are observable only through what the story
  does with them — `TURNS_SINCE`, read counts — which item 1 already
  captures; the debugger sees internals, but the debugger is a
  development surface, not a host);
- the RNG's internal state *as such*.

### 2.1 Consequences stated out loud

- **RNG draw order is protected by construction.** Equivalence must
  hold under every seed, so removing or reordering a `RANDOM` /
  shuffle draw changes every later draw and fails the definition. A
  dead `RANDOM(1, 6)` whose result is unused is *not* removable. This
  is consistent with the runtime already treating a draw as a write to
  the RNG cell (`docs/effects-spec.md`).
- **Save/restore is covered by item 3.** If globals agree at every
  turn boundary, a save taken from either program restores the same
  story. Save *format* is not semantics.
- **Choices compare by order, not by set.** RULED.

### 2.2 A second invariant for source transformations — RULED

Runtime semantics is not the only thing an author has shipped. XLIFF
units and `.inkl` overlays bind to line-table identity (scope ids and
text hashes; `docs/intl-spec.md`). A source transformation that
preserves the trace but shifts identity silently orphans translations.
So every transformation that claims to be *safe* for an author must
also satisfy:

> **Translation identity.** The line table's identity is unchanged for
> every line the transformation did not itself edit.

Checked by diffing the exported line tables before and after. This is
not part of §2's definition (it is not runtime-observable); it is a
separate obligation on source-level tools, and an optimizer working
below the line table inherits it for free.

### 2.3 Future escape hatch — noted, not built

A global the host must never read could be marked (spelling to be
ruled; `#@internal` is the placeholder) to take it out of item 3,
letting a later optimizer eliminate dead stores to it. Nothing here
depends on it; it is recorded so the optimizer's designer knows the
door exists.

## 3. The mechanical oracle

The definition is executable. The episode harness
(`crates/internal/brink-test-harness`: `explorer.rs`, `diff.rs`)
already explores choice trees under fixed seeds with stubbed externals
and step limits, recording output and choices. Extended with items 2–4
of §2 it computes the full trace, and

    trace_diff(P, Q, runs)

over the same explored runs is the equivalence check. Bounded
exploration cannot *prove* equivalence for an unbounded program; the
oracle has coverage, not completeness — the same standing as the C#
oracle ratchet, and the honest bar. Tracked at #3371.

## 4. The guarantee ladder — RULED

The oracle harness alone is not a strong enough guarantee for an
optimizer ("it's got plenty of stuff, but not nearly enough"). The
tiers below are separate test targets; a transformation earns the
label *safe* / *correct* by passing every tier that exists for it.

| Tier | What | Catches | Status |
|---|---|---|---|
| 0 | Corpus differential: `trace_diff` pre vs post over tier1–3 + native goldens | regressions in shapes someone already wrote down | #3371 |
| 1 | Property testing over **generated** programs (proptest, story-level generator; shrinking to a minimal counterexample story) | bugs in shapes nobody wrote down | #3370 |
| 2 | Pass-level metamorphic properties (idempotence `opt(opt(P)) = opt(P)`, commutation where the pass spec says so, "never removes a write to a host-visible global") | bugs that global equivalence hides behind luck | with the optimizer |
| 3a | **Mutation sensitivity of the oracle itself**: semantic mutants of corpus programs (swap two choices, drop a line, flip a condition, reorder a `LIST`, change a literal, remove an unused draw) must each be detected by `trace_diff` | blind spots in the definition or its instrumentation | #3371 — ships **before** anything is built on the oracle |
| 3b | `cargo-mutants` over the optimizer crate: every mutant killed by tiers 0–2 | tests that don't constrain the code | with the optimizer |
| 4 | Runtime fuzz targets pointed at optimized output | bytecode the VM rejects; step-limit violations | with the optimizer |

Ordering RULED: tier 0 and tier 3a first (the oracle must be trusted
before anything rests on it), then the generator, then 2/3b/4 with the
optimizer. Optimized and non-optimized programs must be observably
identical **in all cases** — "all cases" is what the ladder is for.

### 4.1 The generator — RULED shape

The story-level generator is its own epic (#3370). **`.ink` support is
the more urgent surface**: the ink grammar is its first deliverable;
native follows. For the native half both routes are built — direct
`.brink` generation, and routing generated `.ink` through
`brink-respell` — with the respell route doubling as one more
equivalence property, `trace(P) = trace(respell(P))`. The properties it
serves, each its own target:

- `trace(P) = trace(opt(P))` — the optimizer;
- `trace(P) = trace(fmt(P))` — the formatter;
- `trace(P) = trace(respell(P))` — the respeller;
- `trace(P) = trace(fix(P))` for every *Safe* auto-fix;
- compile-road agreement (`compile_path` vs `brink_environment::compile`).

## 5. What "safe" means for a source transformation

A transformation is **Safe** iff it satisfies §2 (observable
equivalence, checked by §3 over the transformation's fixture runs) and
§2.2 (translation identity). It is nothing weaker: not "the author
would probably agree", not "the compiled bytes match", not "the
diagnostic went away". Transformations that change meaning or lose
text — deleting unreachable code, renaming a shadowing temp — are not
Safe however sensible; the auto-fix spec gives them their own tier that
requires positive intent from the author. Deleting a duplicate `LIST`
item changes item 3's values and is therefore not Safe either.

## 6. Not covered

- No claim about **performance** equivalence; an optimizer may change
  step counts freely (they are unobservable).
- No claim about **error** equivalence beyond the terminal kind: a run
  that faults faults in both or neither, but the fault message is a
  diagnostics-channel matter. Whether *which* fault is observable will
  need ruling when the optimizer can change fault ordering.
- Flow suspension (`Step::Suspended`, `docs/flow-suspension-spec.md`)
  is in the trace as a terminal kind only; wake ordering across
  multiple parked flows is not yet defined here.
