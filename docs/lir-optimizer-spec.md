# The LIR optimization stage

**Status: PROPOSED.** One question blocks this document (§9); the rest is
mechanically provable as a no-op and needs no ruling.

This is the **stage** — the seam, the pass contract, the determinism
rules, the measurements, and the fence that proves an empty stage changes
nothing. It deliberately says as little as possible about what passes
*do*.

- What we intend to optimize: `docs/optimizer-catalogue.md`.
- The first pass, and the root-set questions it raises:
  `docs/optimizer-pass-prune.md`.
- The per-pass contract, the enumerated observable surface and the toggle
  grammar: `docs/optimizer-framework-spec.md` — **deliberately deferred**
  until two real passes have bruised against real constraints (§3.1).

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

**What this seam can and cannot reach.** `lir::Program` holds the
container tree, globals, lists and list items, externals, the name
table, struct shapes, private defs, aliases and the file table. It does
**not** hold line tables — codegen builds those while emitting
`StoryData`. A pass that wants to change what the *artifact* contains,
rather than what the LIR contains, is not a pass on this seam; the
catalogue records which candidates that affects.

## 3. The pass contract — PROPOSED

```rust
pub struct OptConfig {
    /// `None` is the A/B control (§6): the stage is a no-op.
    pub level: OptLevel, // None | Default
}

pub struct PassReport {
    pub name: &'static str,
    pub changed: bool,
    /// Pass-specific counts for `brink compile --explain-opt` and tests.
    pub notes: Vec<(&'static str, usize)>,
}

pub struct OptReport {
    pub passes: Vec<PassReport>,
    pub before: ProgramStats, // §5
    pub after: ProgramStats,
}

pub fn optimize(program: &mut lir::Program, config: &OptConfig) -> OptReport;

struct Pass {
    name: &'static str,
    run: fn(&mut lir::Program) -> PassReport,
}

/// Fixed, explicit, in this order. Empty at bring-up (§7).
const PASSES: &[Pass] = &[];
```

- **Ownership: in place.** A pass takes `&mut Program`. The alternative
  (`fn(Program) -> Program`) buys nothing — every pass we can foresee
  edits a small part of a large tree — and costs a clone per pass in the
  common no-op case. A pass that needs to rebuild a table builds it
  locally and swaps it in.
- **Ordering: a fixed list in code**, as #2336's bias says. Passes run
  once each, in list order, no fixpoint loop; a pass that wants to run
  after another states so by its position.
- **Reports, not side channels.** A pass returns what it did. Nothing in
  the stage prints, logs, or reads the environment (§4).
- **No-op fidelity.** With `OptLevel::None` the function returns without
  touching the program. With `Default` on a program that gives every pass
  nothing to do, the program is **byte-identical after codegen** — the
  fence of §6 depends on this, so a pass must not renumber, reorder or
  canonicalize anything it does not remove.

### 3.1 What this document deliberately does not design

Three things are left open on purpose, because settling them now would
prejudge `optimizer-framework-spec.md` while we have no experience to
settle them *with*:

- **`Pass` carries no metadata** — no declaration of what it preserves or
  perturbs. Adding fields is cheap with one pass and expensive with five.
- **No toggle grammar.** A level dial only; no `--opt-disable=<pass>`
  until there are two passes worth disabling.
- **No claim about the effect trace.** An empty pass list perturbs
  nothing, so the stage can stay silent on whether effect rows
  (`--features effect-trace`) are contract or debug surface.

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
     byte-identical `.inkb`.
   - *Order-insensitivity of inputs*: permuting the order in which
     project files reach lowering (within what the topological order
     allows) must not change any pass's report.

## 5. Measurement — PROPOSED

A pass that cannot be measured cannot be evaluated, and "the artifact got
smaller" is the whole point of most of the catalogue. The stage therefore
reports size before and after, whether or not any pass ran.

```rust
pub struct ProgramStats {
    pub containers: usize,     // whole tree, not just root's children
    pub instructions: usize,   // summed over every container's body
    pub globals: usize,
    pub lists: usize,
    pub list_items: usize,
    pub externals: usize,
    pub name_table: usize,
    pub struct_shapes: usize,
    pub aliases: usize,
}
```

All nine are LIR-level and computable inside `optimize()`.

**Two metrics that matter are NOT here, because the seam cannot see
them:** the number of line-table entries — the **translatable units**, and
the only metric in this system denominated in human cost rather than
bytes — and the `.inkb` size itself. Both exist only after codegen. The
gate (§6) measures them by compiling twice and comparing artifacts;
a pass whose goal is stated in those terms needs its own doc to say
where it can possibly run (`docs/optimizer-catalogue.md`).

## 6. The observable-behavior fence — PROPOSED

1. `RATCHET_EPISODE_COUNT` holds exactly.
2. **`optimizer_parallel_gate`** in `brink-test-harness`, the
   `environment_parallel_gate` shape: every corpus case and native golden
   compiled with `OptLevel::None` and `OptLevel::Default`; `trace_diff`
   must be empty, and for every case whose pass reports say
   `changed: false` the `.inkb` bytes must be identical.
3. `@brink-lang/web`-observable behavior for a project that references
   everything it mounts is byte-identical (a consequence of 2 plus §3's
   no-op fidelity).
4. **The generator is the real fence.** `trace(compile(P, Default)) ==
   trace(compile(P, None))` over `brink-gen`'s stories explores shapes
   nobody wrote, which golden A/B cannot. Each pass brings a generator
   knob that emits its input shape, and its property asserts both halves:
   the traces agree **and** the pass's metric actually moved. The second
   half is what catches a pass that silently does nothing. See the
   catalogue.

With `PASSES` empty, 1–3 are provable by construction and 4 is vacuous —
which is exactly why bring-up starts there.

## 7. Bring-up order

1. Land the stage with `PASSES` empty, `ProgramStats` reported, and the
   gate green. This proves the seam is a no-op and gives the plumbing a
   user on day one.
2. Land the first pass behind `OptLevel::Default`
   (`docs/optimizer-pass-prune.md`).
3. Flip the interim (§8).

## 8. The interim, and what removes it

`docs/stdlib-spec.md` and the 2026-08-06 decision-log entry record the
mount pollution as accepted: every `.inkb` built from a **native**
project carries the whole mounted `std/` today. (An ink project does
not: `brink_environment::collect_sources` gives ink the `INCLUDE`-
reachable set from the entry, and the mount is `.brink` files an `.ink`
entry can never reach. The pollution is native-only.)

This document is step 1 of the removal; the prune doc's ruling is step 2;
the stage + prune + gate is step 3; the last step edits the two documents
to say the pollution is gone and retires #2228's evidence note.

## 9. The one question this document needs ruled

**Exposure of `OptLevel`.** The A/B gate cannot exist without a way to
select the level, so this must be settled before the stage lands.
Proposal: a `brink compile --opt none` flag, **mount-time only, never
embedded in `.inkb`**. Alternative: also a `[build] opt = "none"` key in
`brink.toml`. Proposal favours the flag alone in v1 — a project-level
key invites shipping an artifact whose provenance is unclear.

The prune's four questions (`use` as a root, whether project definitions
are ever pruned, the pruned-name list, id stability over compaction) now
live in `docs/optimizer-pass-prune.md`, and do not block this document.
