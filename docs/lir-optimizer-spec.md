# LIR optimization stage — pass contract, determinism, the prune's root set

Status: **PROPOSED (needs ruling)** — drafted 2026-09-04 against issue
#2336, which owes this document to the 2026-08-06 ruling *"LIR
optimization stage; reachability prune is its first pass"*
(`docs/decision-log.md`). Nothing here is implemented; per the issue,
**do not implement from this document until it is ruled**. Sections marked
RULED restate the 2026-08-06 ruling and are not up for re-litigation here;
everything else is a proposal with its alternatives named, and §9 lists
the questions the ruling has to answer.

## 1. What is ruled (restated, not re-argued)

- The pipeline gains a general `LIR → passes → LIR` stage between LIR
  lowering and bytecode codegen.
- Its first resident pass is **reachability pruning**: the stdlib mount
  stays universal and unconditional (the environment-is-the-universe
  model, `docs/modules-spec.md`), but codegen emits only definitions
  reachable from the artifact's roots. Unreferenced stdlib content must
  not reach `StoryData` (#2228).
- Until the pass lands, the mount pollution is an accepted, documented
  interim (§8).
- Mount-on-demand stays rejected.
- Each pass is a pure function of the program with no iteration-order
  dependence.

## 2. Where the stage sits

Today the seam is `brink-compiler`'s `compile` (and the Environment
road's `brink_environment::compile`): HIR files → `brink_ir::lir::lower`
→ `lir::Program` → `brink_codegen_inkb` → `StoryData`. The stage is one
call on that seam, on both roads, so `environment_parallel_gate`'s
"the two roads agree" property keeps covering it:

```text
lir::Program  ──▶  lir::opt::optimize(&mut program, &OptConfig)  ──▶  codegen
```

It lives in `brink-ir` as `lir::opt` (the passes read and write LIR
types and nothing else); the driver crates only choose the config.

## 3. The pass contract — PROPOSED

```rust
pub struct OptConfig {
    /// `None` is the A/B control (§7): the stage is a no-op.
    pub level: OptLevel, // None | Default
}

pub struct PassReport {
    pub name: &'static str,
    pub changed: bool,
    /// Pass-specific counts for `brink compile --explain-opt` and tests
    /// (the prune reports definitions removed, by kind).
    pub notes: Vec<(&'static str, usize)>,
}

pub fn optimize(program: &mut lir::Program, config: &OptConfig) -> Vec<PassReport>;

struct Pass {
    name: &'static str,
    run: fn(&mut lir::Program) -> PassReport,
}

/// Fixed, explicit, in this order. No registry, no plugins, no
/// feature-gated entries: a pass is added by editing this list.
const PASSES: &[Pass] = &[Pass { name: "prune-unreachable", run: prune::run }];
```

- **Ownership: in place.** A pass takes `&mut Program`. The alternative
  (`fn(Program) -> Program`) buys nothing — every pass we can foresee
  edits a small part of a large tree — and costs a clone per pass in the
  common no-op case. A pass that needs to rebuild a table builds it
  locally and swaps it in.
- **Ordering: a fixed list in code**, as #2336's bias says. Passes run
  once each, in list order, no fixpoint loop; a pass that wants to run
  after another states so by its position. A second real pass is the
  earliest moment to revisit this.
- **Reports, not side channels.** A pass returns what it did. Nothing in
  the stage prints, logs, or reads the environment (§4). `brink compile
  --explain-opt` (later) renders the reports; tests assert on them.
- **No-op fidelity.** With `OptLevel::None` the function returns without
  touching the program. With `Default` on a program that gives every pass
  nothing to do, the program is **byte-identical after codegen** — the
  no-op fence of §7 depends on this, so a pass must not renumber,
  reorder or canonicalize anything it does not remove.

## 4. Determinism rules — PROPOSED

1. A pass is a pure function of `lir::Program` (plus its `OptConfig`).
   No clock, no environment variable, no file, no global state, no
   randomness.
2. Iteration is over `Vec`s in program order and over `BTreeMap` /
   `BTreeSet`; a `HashMap`/`HashSet` may be used only as an index that
   is never iterated (the house rule; the analyzer's label-lookup and
   the db's file-ordering bugs are the precedent).
3. Removal preserves relative order of everything kept: the surviving
   `children`, `globals`, `lists`, `externals`, `aliases` and name-table
   entries keep their original order. A pass never allocates fresh
   `DefinitionId`s or `NameId`s.
4. **Stability is tested, not assumed.** Three checks, all in
   `crates/internal/brink-test-harness`:
   - *Idempotence*: `opt(opt(P)) = opt(P)` — the second run reports
     `changed: false` for every pass and the `.inkb` bytes are equal.
   - *Run-to-run*: two independent compilations of the same source yield
     byte-identical `.inkb` (already the environment-parallel gate's
     shape; the stage adds nothing to it but is covered by it).
   - *Order-insensitivity of inputs*: for the prune, permuting the order
     in which project files are handed to lowering (within what the
     topological order allows) must not change the set of definitions
     removed — checked on the corpus by comparing the pass reports.

## 5. Reachability pruning — the root set — PROPOSED

This is the hard part #2336 names. The proposed rule, in one line:

> **A definition is shipped iff it is declared by the project, or it is
> reachable from something the project declares. Mounted library
> definitions are shipped only when the project reaches them.**

### 5.1 Roots

1. **The story entry**: the root container and whatever the entry divert
   names.
2. **Every definition declared in a project file**, `#@private` or not:
   containers (knots, stitches, functions, labels), globals, lists and
   their items, externals and their fallbacks, struct shapes, aliases
   (`#@was`). The prune **never removes project definitions** (§5.4 says
   why); so "root" here means "and everything they reach is kept too".
3. **Explicit imports** of library symbols (`use std::…`), even when
   nothing calls them. An import is the author saying "I mean to use
   this"; a host that calls it by name through `Story::call_function` /
   `begin_function_eval` / `choose_path_string` after the author imported
   it must find it. This is the answer to "does the engine seam make
   roots more than the entry point?": the engine's callable surface is
   *the project's non-private definitions plus what it imports*, not the
   whole mount.

Not roots: mounted definitions nobody references. That includes
library functions a host might *want* to call without the project
naming them — §5.5 says what happens then.

### 5.2 Edges

From a kept definition, everything its LIR mentions is reachable:

| LIR site | edge to |
|---|---|
| `Divert`, `TunnelCall`, `ThreadCall`, gather/choice targets | the target container |
| `Call`, `CallExternal`, `CallVariable`, `MakeFnValue` | the callee (and an external's fallback) |
| `DivertTarget(id)` as a value, `VisitCount(id)`, `TURNS_SINCE`-style reads | the container — a value that names a definition keeps it, since `-> variable` diverts and visit-count reads resolve at runtime |
| `GetGlobal`/`SetGlobal`/`TakeGlobal`, `ListLiteral { items, origins }` | the global; the list and its items |
| struct-shape references, `#fn` element bindings | the shape / the handler |
| `aliases` (`#@was`) | an alias whose target is kept is kept |
| nested `children` | a kept container keeps its children (they are addressed through it) |

A conservative rule for anything not in the table: **if a `DefinitionId`
appears in kept LIR, its definition is kept.** The walk is a worklist
over `DefinitionId`s with a `BTreeSet` of seen ids; unknown ids (a
dangling reference lowering already diagnosed) are ignored, never
fatal.

### 5.3 What is removed

Only mounted-library definitions (those whose `file_paths` entry is the
mount, i.e. `std/…`) that the walk never reaches: their containers (with
children), globals, lists/items, externals, struct shapes, aliases, and
the name-table entries and line-table entries nothing kept references.
The name table is compacted **without renumbering kept ids** (a
tombstone-free table would renumber; renumbering would change
`.inkb` bytes for projects that reference everything, breaking §7's
no-op fence — so v1 keeps ids stable and lets the name table carry
unused slots, exactly as today's artifacts already may).

### 5.4 Why project definitions are never pruned (v1)

The pass exists to stop the mount polluting artifacts, not to
dead-code-eliminate the author's program. Pruning an unreachable
project function would remove its line-table entries (the translation
surface `docs/intl-spec.md` walks — a translator's unit vanishing
because a knot is temporarily unreachable is exactly the "silent
drop" the rules forbid), its debugger addresses, and its
host-callability. Dead project code is a *lint*'s business (E-codes
already exist for unreachable content), never the optimizer's. A later
pass may revisit private, unreferenced project globals under
`docs/observable-semantics-spec.md`'s dead-store clause, on its own
ruling.

### 5.5 A host call to a pruned library symbol

`Program::find_address` returns `None` and `choose_path_string` /
`call_function` return `RuntimeError::UnknownPath` today. That stays
the outcome, made diagnosable: the runtime keeps, in `StoryData`, the
list of **pruned library names** (names only — a few hundred bytes),
and `UnknownPath`'s message says *"`std::x` was not shipped: the
project never references it (reachability prune); import it with `use`
to ship it"* when the name is on that list. No fallback mount, no
lazy load: the ruling rejected usage-dependent universes at analysis
time and this document does not reintroduce them at load time.

### 5.6 Saves, visit counts, and state

Save state keys visit counts and turn counts by definition path. A
pruned definition was never reachable, so no save produced by this
artifact can name it. A save produced by an **earlier build** that
still shipped the definition can: `LoadReport` already reports unknown
keys (the `#@was` rename machinery's own path), and a pruned name lands
there as an ordinary "unknown definition" entry, which is the right
answer — the definition is gone from this artifact by construction, not
by rename.

## 6. Tests the pass ships with — PROPOSED

- **Unit**: a mount with two library functions, a project referencing
  one → the other is pruned; the same with an explicit `use` of the
  other → both kept; a `DivertTarget` value naming a library knot keeps
  it; an external's fallback is kept with the external.
- **Tier 0 fence** (§7): the corpus and native goldens, A/B.
- **Idempotence** and the two other stability checks of §4.
- **Generator**: `brink-gen`'s profiles gain an `imports` knob only when
  the native printer lands (`docs/program-generator-spec.md` §7); until
  then the ink profile is the no-op case by construction (ink projects
  reference no mount), which is itself a fence.

## 7. The observable-behavior fence — PROPOSED

1. `RATCHET_EPISODE_COUNT` holds exactly (an ink project references no
   library symbol, so the prune is a no-op there — and the fence proves
   it rather than assuming it).
2. **`optimizer_parallel_gate`** in `brink-test-harness`, the
   `environment_parallel_gate` shape: every corpus case and native golden
   compiled with `OptLevel::None` and `OptLevel::Default`; `trace_diff`
   must be empty, and for every case whose pass reports say `changed:
   false` the `.inkb` bytes must be identical.
3. `@brink-lang/web`-observable behavior for a project that references
   everything it mounts is byte-identical (a consequence of 2 plus §3's
   no-op fidelity).
4. Bring-up order: land the stage with `PASSES` empty and the gate
   green (proves the seam is a no-op), then the prune behind
   `OptLevel::Default`, then flip the interim (§8).

## 8. The interim, and what removes it

`docs/stdlib-spec.md` and the 2026-08-06 decision-log entry record the
mount pollution as accepted: every `.inkb` built from a native project
carries the whole mounted `std/` today. This document is step 1 of the
removal; a ruling on §9 is step 2; the stage + prune + gate is step 3;
the last step edits the two documents to say the pollution is gone and
retires #2228's evidence note.

## 9. Questions for the ruling

1. **`use` as a root** (§5.1.3): an explicit import ships the symbol
   even when uncalled. Alternative: only calls/diverts count, and a
   host that needs an uncalled library function references it from a
   `pub` project stub. Proposal favours the import.
2. **Project definitions are never pruned in v1** (§5.4). Alternative:
   prune unreachable `#@private` project definitions too. Proposal says
   no — that is a lint's job and it touches the translation surface.
3. **The pruned-name list in `StoryData`** (§5.5) for the diagnosable
   error. Alternative: a bare `UnknownPath`. Proposal favours the list
   (small, and it turns a mystery into a one-line fix).
4. **Exposure of `OptLevel`**: `brink compile --opt none` and a
   `[build] opt = "none"` key in `brink.toml`, both mount-time only and
   never embedded in `.inkb` — needed for the A/B gate and for a
   maintainer bisecting a suspected pass. Proposal: CLI flag only in v1.
5. **Id stability over compaction** (§5.3): keep ids, tolerate unused
   name-table slots. Alternative: renumber and accept that the no-op
   fence becomes "trace-equal" rather than "byte-identical" for every
   native project. Proposal keeps ids.
