# Pass: prune unreachable definitions

**Status: PROPOSED (needs ruling).** Four questions in §6 must be
answered before this is implemented. The stage it runs on is
`docs/lir-optimizer-spec.md`; the catalogue entry is
`docs/optimizer-catalogue.md`.

This is the first per-pass document, and it follows the template every
later one should: **behavior**, **constraints**, **generator**, then the
open questions.

## 1. Behavior

The pass exists to stop the universal `std/` mount polluting native
artifacts (#2228). In one line:

> **A definition is shipped iff it is declared by the project, or it is
> reachable from something the project declares. Mounted library
> definitions are shipped only when the project reaches them.**

Ink projects are unaffected — they never carry the mount at all
(`docs/lir-optimizer-spec.md` §8), so for them this pass is a no-op by
construction, which makes them a useful control rather than a target.

### 1.1 Roots

1. **The story entry**: the root container and whatever the entry divert
   names.
2. **Every definition declared in a project file**, `#@private` or not:
   containers (knots, stitches, functions, labels), globals, lists and
   their items, externals and their fallbacks, struct shapes, aliases
   (`#@was`). The prune **never removes project definitions** (§2.1 says
   why); "root" here means "and everything they reach is kept too".
3. **Explicit imports** of library symbols (`use std::…`), even when
   nothing calls them. An import is the author saying "I mean to use
   this"; a host that calls it by name through `Story::call_function` /
   `begin_function_eval` / `choose_path_string` after the author imported
   it must find it. This is the answer to "does the engine seam make
   roots more than the entry point?": the engine's callable surface is
   *the project's non-private definitions plus what it imports*, not the
   whole mount.

Not roots: mounted definitions nobody references — including library
functions a host might *want* to call without the project naming them.
§2.3 says what happens then.

### 1.2 Edges

From a kept definition, everything its LIR mentions is reachable:

| LIR site | edge to |
|---|---|
| `Divert`, `TunnelCall`, `ThreadCall`, gather/choice targets | the target container |
| `Call`, `CallExternal`, `CallVariable`, `MakeFnValue` | the callee (and an external's fallback) |
| `DivertTarget(id)` as a value, `VisitCount(id)`, `TURNS_SINCE`-style reads | the container — a value that names a definition keeps it, since `-> variable` diverts and visit-count reads resolve at runtime |
| `GetGlobal`/`SetGlobal`/`TakeGlobal`, `ListLiteral { items, origins }` | the global; the list and its items |
| struct-shape references, `#fn` element bindings | the shape / the handler |
| `aliases` (`#@was`) | an alias whose target is kept is kept |
| nested `children` | a kept container keeps its children |

A conservative rule for anything not in the table: **if a `DefinitionId`
appears in kept LIR, its definition is kept.** The walk is a worklist
over `DefinitionId`s with a `BTreeSet` of seen ids; unknown ids (a
dangling reference lowering already diagnosed) are ignored, never fatal.

### 1.3 What is removed

Only mounted-library definitions (those whose `file_paths` entry is the
mount, i.e. `std/…`) that the walk never reaches: their containers (with
children), globals, lists/items, externals, struct shapes, aliases, and
the name-table entries nothing kept references.

The name table is compacted **without renumbering kept ids**. A
tombstone-free table would renumber, and renumbering changes `.inkb`
bytes for projects that reference everything — which breaks the stage's
byte-identical no-op fence. v1 keeps ids stable and lets the table carry
unused slots, as today's artifacts already may.

## 2. Constraints

What this pass must not disturb, and why.

### 2.1 The translation surface — project definitions are never pruned

Pruning an unreachable *project* function would remove its line-table
entries, and those are the units `docs/intl-spec.md` walks. A
translator's unit vanishing because a knot is temporarily unreachable is
exactly the silent drop the house rules forbid. It would also remove the
definition's debugger addresses and its host-callability.

Dead project code is a **lint's** business — E-codes for unreachable
content already exist — never the optimizer's. A later pass may revisit
private, unreferenced project globals under
`docs/observable-semantics-spec.md`'s dead-store clause, on its own
ruling.

Note the boundary this draws, because a sibling pass tests it: the
constraint is *never lose a translatable unit*, not *never touch the
line table*. Deduplicating identical units does not lose any
(`docs/optimizer-catalogue.md`).

### 2.2 Saves, visit counts, and state

Save state keys visit counts and turn counts by definition path. A
pruned definition was never reachable, so no save produced by this
artifact can name it. A save produced by an **earlier build** that still
shipped the definition can: `LoadReport` already reports unknown keys
(the `#@was` rename machinery's own path), and a pruned name lands there
as an ordinary "unknown definition" entry — which is the right answer,
since the definition is gone by construction rather than by rename.

### 2.3 A host call to a pruned library symbol

`Program::find_address` returns `None` and `choose_path_string` /
`call_function` return `RuntimeError::UnknownPath` today. That stays the
outcome, made diagnosable: `StoryData` keeps the list of **pruned
library names** (names only — a few hundred bytes), and `UnknownPath`'s
message says *"`std::x` was not shipped: the project never references it
(reachability prune); import it with `use` to ship it"* when the name is
on that list.

No fallback mount, no lazy load: the ruling rejected usage-dependent
universes at analysis time and this does not reintroduce them at load
time.

### 2.4 Id stability

Covered in §1.3 — it is a constraint the *stage* imposes (the
byte-identical fence), not one this pass chooses.

## 3. Generator

The knob this pass needs does not exist yet, and cannot until the native
printer lands (`docs/program-generator-spec.md` §7): the pass only does
anything for a native project with a mount, and `brink-gen` prints ink
today.

Until then the ink profile **is** the no-op case by construction, which
is itself a fence — every generated story exercises "the prune finds
nothing to do and the artifact is byte-identical".

When the native printer lands, the knob is `imports`: emit projects that
reference some, none, or all of the mounted surface, plus projects with
an explicit `use` of a symbol nothing calls (the §6.1 question made
executable). The property is the catalogue's standard pair — traces
agree, and `ProgramStats` actually moved for the projects that should
shrink.

## 4. Tests it ships with

- **Unit**: a mount with two library functions and a project referencing
  one → the other is pruned; the same with an explicit `use` of the other
  → both kept; a `DivertTarget` value naming a library knot keeps it; an
  external's fallback is kept with the external.
- **Fence**: the stage's `optimizer_parallel_gate` over the corpus and
  native goldens, A/B.
- **Stability**: the stage's three determinism checks.

## 5. What this pass does not do

It is native-facing. For an ink project it is permanently a no-op, and
nothing in the catalogue's other entries changes that — see the
catalogue for what an ink-facing optimizer would need.

## 6. Questions for the ruling

1. **`use` as a root** (§1.1.3): an explicit import ships the symbol even
   when uncalled. Alternative: only calls and diverts count, and a host
   that needs an uncalled library function references it from a `pub`
   project stub. Proposal favours the import.
2. **Project definitions are never pruned in v1** (§2.1). Alternative:
   prune unreachable `#@private` project definitions too. Proposal says
   no — a lint's job, and it touches the translation surface.
3. **The pruned-name list in `StoryData`** (§2.3) for the diagnosable
   error. Alternative: a bare `UnknownPath`. Proposal favours the list —
   small, and it turns a mystery into a one-line fix.
4. **Id stability over compaction** (§1.3): keep ids, tolerate unused
   name-table slots. Alternative: renumber and accept that the no-op
   fence weakens to trace-equality for every native project. Proposal
   keeps ids.
