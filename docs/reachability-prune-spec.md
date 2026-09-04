# Reachability pruning — a compiler emission step

**Status: PROPOSED (needs ruling).** Four questions in §7, plus an
open structural question in §6.

The compiler must not ship the parts of the universal `std/` mount a
project never reaches (#2228). This is **not** an optimizer pass: it is
the compiler deciding what belongs in the artifact at all
(`docs/optimizer-spec.md` §7 draws that boundary). The 2026-08-06
ruling's own wording — *codegen emits only definitions reachable from
the artifact's roots* — describes exactly this step.

It is also the **first inhabitant of the compiler's own LIR transform
layer**, and 08-06's `LIR → passes → LIR` mechanism is meant for that
layer. Constant folding and dead-branch elimination — the transforms
that ruling named as needing somewhere to live — belong beside this one:
they need types, provenance and lowering's invariants, so they can never
move to the post-compile optimizer. §6 keeps that question open rather
than answering it from a sample of one.

## 1. Behavior

> **A definition is shipped iff it is declared by the project, or it is
> reachable from something the project declares. Mounted library
> definitions are shipped only when the project reaches them.**

**Where it runs.** Reachability is computed over `lir::Program` and
applied before codegen. That placement is not incidental: the walk needs
to tell project definitions from mounted ones, and only the compiler
always knows — `lir::Program` carries the file table, while the
artifact's `debug_info` is `Option` and absent from release builds. This
is the reason the prune stays here while the optimizer moved to the
artifact.

**It is unconditional.** Not shipping the mount is correctness of
emission, not a tuning choice, so there is no level to set. Whether it
sits behind a general pass mechanism is a separate question (§6) — being
unconditional does not require being a one-off hook.

**Ink projects are unaffected**, and not because they reference nothing:
`brink_environment::collect_sources` gives an ink entry the
`INCLUDE`-reachable set, and the mount is `.brink` files an `.ink` entry
can never reach. Ink never carries the mount, so this step finds nothing
to do — which makes ink a useful control.

### 1.1 Roots

1. **The story entry**: the root container and whatever the entry divert
   names.
2. **Every definition declared in a project file**, `#@private` or not:
   containers (knots, stitches, functions, labels), globals, lists and
   their items, externals and their fallbacks, struct shapes, aliases
   (`#@was`). Project definitions are **never removed** (§2.1); "root"
   here means "and everything they reach is kept too".
3. **Explicit imports** of library symbols (`use std::…`), even when
   nothing calls them. An import is the author saying "I mean to use
   this"; a host calling it by name through `Story::call_function` /
   `begin_function_eval` / `choose_path_string` must find it. This is the
   answer to "does the engine seam make roots more than the entry
   point?": the callable surface is *the project's non-private
   definitions plus what it imports*, not the whole mount.

Not roots: mounted definitions nobody references, including library
functions a host might want to call without the project naming them.
§2.3 says what happens then.

### 1.2 Edges

From a kept definition, everything its LIR mentions is reachable:

| LIR site | edge to |
|---|---|
| `Divert`, `TunnelCall`, `ThreadCall`, gather/choice targets | the target container |
| `Call`, `CallExternal`, `CallVariable`, `MakeFnValue` | the callee (and an external's fallback) |
| `DivertTarget(id)` as a value, `VisitCount(id)`, `TURNS_SINCE`-style reads | the container — a value naming a definition keeps it, since `-> variable` and visit-count reads resolve at runtime |
| `GetGlobal`/`SetGlobal`/`TakeGlobal`, `ListLiteral { items, origins }` | the global; the list and its items |
| struct-shape references, `#fn` element bindings | the shape / the handler |
| `aliases` (`#@was`) | an alias whose target is kept is kept |
| nested `children` | a kept container keeps its children |

Conservative rule for anything not in the table: **if a `DefinitionId`
appears in kept LIR, its definition is kept.** The walk is a worklist
over `DefinitionId`s with a `BTreeSet` of seen ids; unknown ids (a
dangling reference lowering already diagnosed) are ignored, never fatal.

### 1.3 What is removed

Only mounted-library definitions (those whose `file_paths` entry is the
mount, `std/…`) the walk never reaches: their containers with children,
globals, lists and items, externals, struct shapes, aliases, and the
name-table entries nothing kept references.

Kept ids are **not renumbered**. Renumbering would churn every artifact
built from a project that references everything, for no benefit, and it
would make this step's output impossible to diff against a build without
it during bring-up (§5). The name table keeps unused slots, as today's
artifacts already may.

## 2. Constraints

### 2.1 The translation surface — project definitions are never pruned

Pruning an unreachable *project* function would remove its line-table
entries, and those are the units `docs/intl-spec.md` walks. A
translator's unit vanishing because a knot is temporarily unreachable is
the silent drop the house rules forbid. It would also take the
definition's debugger addresses and host-callability with it.

Dead project code is a **lint's** business — E-codes for unreachable
content already exist — never emission's.

Note the boundary, because a sibling transform tests it: the constraint
is *never lose a translatable unit*, not *never touch the line table*.
Deduplicating identical units loses none, which is why that one is a
legitimate optimizer pass (`docs/optimizer-catalogue.md`).

### 2.2 Saves, visit counts, and state

Save state keys visit and turn counts by definition path. A pruned
definition was never reachable, so no save from this artifact can name
it. A save from an **earlier build** that still shipped it can:
`LoadReport` already reports unknown keys (the `#@was` machinery's path),
and a pruned name lands there as an ordinary unknown-definition entry —
the right answer, since it is gone by construction rather than by rename.

### 2.3 A host call to a pruned library symbol

`Program::find_address` returns `None` and `choose_path_string` /
`call_function` return `RuntimeError::UnknownPath` today. That stays,
made diagnosable: `StoryData` keeps the list of **pruned library names**
(names only — a few hundred bytes), and `UnknownPath` says *"`std::x` was
not shipped: the project never references it; import it with `use` to
ship it"* when the name is on that list.

No fallback mount, no lazy load — the ruling rejected usage-dependent
universes at analysis time and this does not reintroduce them at load
time.

## 3. Tests

- **Unit**: a mount with two library functions and a project referencing
  one → the other is not shipped; the same with an explicit `use` of the
  other → both shipped; a `DivertTarget` value naming a library knot
  keeps it; an external's fallback is kept with the external.
- **Behavioral**: the corpus and native goldens are unchanged. Since the
  step is unconditional there is no permanent A/B, so bring-up carries a
  temporary `--no-prune` (§5) and the fence is "artifacts built with and
  without it trace identically". The flag goes away when the step is
  trusted.
- **`RATCHET_EPISODE_COUNT` holds exactly** — an ink project has no mount
  to prune, so the ratchet proves the step is inert where it should be.

## 4. Generator

The knob cannot exist until the native printer lands
(`docs/program-generator-spec.md` §7): the step only does anything for a
native project with a mount, and `brink-gen` prints ink today. Until
then every generated story exercises the inert case, which is itself a
fence.

When the native printer lands, the knob is `imports`: projects
referencing some, none, or all of the mounted surface, plus projects with
an explicit `use` of a symbol nothing calls — §6.1's question made
executable.

## 5. Bring-up

1. Land the reachability walk behind a temporary `--no-prune` escape
   hatch, defaulting to on.
2. Prove artifacts trace identically with and without it across the
   corpus and native goldens, and that native artifacts actually shrink.
3. Remove the flag and flip the interim: `docs/stdlib-spec.md` and the
   2026-08-06 decision-log entry record the mount pollution as accepted,
   and both need editing to say it is gone, retiring #2228's evidence
   note.

## 6. Does this layer get a pass mechanism, and when?

Open, deliberately. 08-06 ruled that whole-program compiler work needs a
general stage rather than a one-off hook, and the reason was foresight
about constant folding and dead-branch elimination, not about the prune.
With one transform in hand, both readings are defensible:

- **Build the mechanism now.** It is cheapest with one inhabitant, and
  the ruling already asked for it. The risk is designing a pass contract
  from a sample of one — the same trap `docs/optimizer-framework-spec.md`
  is deferred to avoid.
- **Build a single step now, generalise on the second transform.**
  Converting one step into the first entry of a pass list is a small,
  mechanical change, and by then there is a second inhabitant to design
  against.

Nothing in this document depends on the answer: the reachability walk,
its roots, its edges and its constraints are identical either way. The
choice only decides where the code sits.

## 7. Questions for the ruling

1. **`use` as a root** (§1.1.3): an explicit import ships the symbol even
   when uncalled. Alternative: only calls and diverts count, and a host
   needing an uncalled library function references it from a `pub`
   project stub. Proposal favours the import.
2. **Project definitions are never pruned** (§2.1). Alternative: prune
   unreachable `#@private` project definitions too. Proposal says no — a
   lint's job, and it touches the translation surface.
3. **The pruned-name list in `StoryData`** (§2.3). Alternative: a bare
   `UnknownPath`. Proposal favours the list — small, and it turns a
   mystery into a one-line fix.
4. **Id stability** (§1.3): keep ids and tolerate unused name-table
   slots. Alternative: renumber and compact. Proposal keeps ids, so
   bring-up can diff artifacts built with and without the step.
