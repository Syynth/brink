# The story optimizer

**Status: PROPOSED (needs ruling).** This supersedes part of the
2026-08-06 ruling — see §1.

The optimizer is a **post-compile `.inkb` → `.inkb` transform**. It makes
a shipped artifact smaller and cheaper without changing what it does. It
is not part of compilation.

- What we intend it to do: `docs/optimizer-catalogue.md`.
- The per-pass contract, the enumerated observable surface, the toggle
  grammar: `docs/optimizer-framework-spec.md` — deferred until two real
  passes have pushed on them.
- Not here: the reachability prune, which is a **compiler** step
  (`docs/reachability-prune-spec.md`).

## 1. What this supersedes, and what it does not

The 2026-08-06 ruling said:

> The pipeline gains a general optimization stage between LIR lowering
> and codegen (`LIR → passes → LIR`). Its first resident pass is
> reachability pruning … codegen emits only definitions reachable from
> the artifact's roots.

**Preserved in full:** the mount stays universal; the shipped artifact
carries only what the project reaches; mount-on-demand stays rejected;
each transform is a pure function with no iteration-order dependence.
The ruling's own wording — *codegen emits only definitions reachable* —
already describes an emission step, and that is what the prune becomes.

**Not superseded, but re-scoped:** the mechanism. `LIR → passes → LIR`
remains available to the **compiler**, and that ruling's reason for
wanting it — "future whole-program work (constant folding, dead-branch
elimination) otherwise has nowhere to live" — still holds for exactly
those transforms, which need types, provenance and lowering's invariants
and so can never be done post-compile. What changes is only that **the
optimizer is not that stage**: it does not live at LIR, and the prune is
not one of its passes. Whether the compiler's side generalises into a
pass list now or stays a single step until a second transform arrives is
an open question for `docs/reachability-prune-spec.md`, not this
document.

Why the placement moved: of five candidate optimizations, four operate on
things that **do not exist at LIR**.

| Candidate | Operates on | At LIR? |
|---|---|---|
| peephole | bytecode | impossible |
| line-table dedup | line tables | built by codegen |
| literal-pool / name-table compaction | the pools | built by codegen |
| redundant pure work | effect rows | `effect_rows` ship in `StoryData` |
| reachability prune | definitions + provenance | yes — and it stays in the compiler |

Two facts settled it. `StoryData` carries `effect_rows`, so the one pass
whose evidence I assumed was compiler-only works fine on the artifact.
And the fence is far stronger post-compile: the control is the
**untouched artifact, byte for byte**, rather than a second compilation
that has to be proven equivalent through two compile roads.

## 2. Shape

```text
brink compile  ──▶  story.inkb  ──▶  brink opt  ──▶  story.inkb
```

A crate, `brink-opt`, depending on `brink-format` and nothing else — it
reads and writes `StoryData` and has no opinion about how the artifact
was produced. That independence is the point: it works on an artifact
from an older compiler, and the compiler can be restructured beneath it.

```rust
pub struct OptConfig { pub passes: PassSet }

pub struct PassReport {
    pub name: &'static str,
    pub changed: bool,
    pub notes: Vec<(&'static str, usize)>,
}

pub struct OptReport {
    pub passes: Vec<PassReport>,
    pub before: ArtifactStats,
    pub after: ArtifactStats,
}

pub fn optimize(story: &mut StoryData, config: &OptConfig) -> OptReport;
```

Passes are a fixed list in code, run once each in list order, no fixpoint
loop. A pass takes `&mut StoryData`: every foreseeable pass edits a small
part of a large structure, and a `fn(StoryData) -> StoryData` costs a
clone per pass in the common no-op case.

## 3. Measurement

The placement's quiet advantage: **every metric that matters is
measurable in one place**, because it all lives in the artifact.

```rust
pub struct ArtifactStats {
    pub containers: usize,
    pub bytecode_bytes: usize,
    pub line_entries: usize,   // translatable units
    pub name_table: usize,
    pub literal_pool: usize,
    pub list_literals: usize,
    pub artifact_bytes: usize,
}
```

`line_entries` is the only figure here denominated in human cost rather
than machine cost — it is what a translator is billed for — and it was
not measurable at all under the previous placement.

## 4. Determinism

1. A pass is a pure function of `StoryData` plus its config. No clock, no
   environment, no file, no global state, no randomness.
2. Iteration is over `Vec`s in program order and over `BTreeMap` /
   `BTreeSet`; a `HashMap` may be used only as an index that is never
   iterated (the house rule).
3. A pass never renumbers a `DefinitionId` or `NameId` it keeps.
4. Tested, not assumed: **idempotence** (`opt(opt(A)) == opt(A)`,
   byte-identical) and **run-to-run stability** (two runs over the same
   input produce identical bytes).

## 5. The fence

1. **Trace equality.** For every corpus case and native golden: compile
   once, optimize a copy, run both artifacts through the runtime, and
   require identical episodes. The control is the input artifact itself —
   no second compilation, no config threaded through two compile roads,
   no parallel gate.
2. **The generator.** `trace(opt(compile(P))) == trace(compile(P))` over
   `brink-gen`'s stories explores shapes nobody wrote. Each pass brings a
   knob emitting its input shape, and asserts the standard pair: traces
   agree **and** the pass's metric moved. The second half catches a pass
   that silently does nothing.
3. **`RATCHET_EPISODE_COUNT` is untouched** — the corpus compiles without
   the optimizer, so the ratchet cannot move either way. That is a
   feature of the placement, not a gap: conformance and optimization stop
   sharing a number.

## 6. What the artifact cannot tell us

Stated so nobody designs a pass that needs it:

- **Types.** Bytecode is largely untyped, so type-directed optimization
  is out of scope here by construction. Anything needing types belongs in
  the compiler.
- **Source provenance, sometimes.** `debug_info` is
  `Option<DebugInfoSection>` and may be absent in a release build. A pass
  must not depend on knowing which source file a definition came from.
  (This is precisely why the prune is a compiler step: it needs to tell
  project definitions from mounted ones, and only the compiler always
  knows.)
- **Lowering's structural invariants**, which are guarantees about LIR,
  not about the artifact.

## 7. The boundary

> **The compiler decides what to ship. The optimizer makes what ships
> cheaper without changing what it does.**

Reachability pruning is the compiler deciding what to ship, which is why
it moved out of this document. If a future transform is answering *what
belongs in the artifact*, it is a compiler transform; if it is answering
*how cheaply the same artifact can say the same thing*, it is a pass
here.

That test puts constant folding and dead-branch elimination on the
compiler's side too — they change what the artifact says, using type and
provenance information this crate cannot see. They are not in this
document's catalogue, and the mechanism they will want is 08-06's.

## 8. Bring-up order

1. Land `brink-opt` with an empty pass list, `ArtifactStats` reported,
   and the fence green. An empty optimizer is provably byte-identical, so
   this proves the harness rather than the passes.
2. Land the first pass (`docs/optimizer-catalogue.md` — line-table dedup
   is the obvious candidate: the biggest win, the clearest metric).
3. Revisit `optimizer-framework-spec.md` once a second pass exists.

## 9. Questions for the ruling

1. **Does `brink compile` run the optimizer by default?** Proposal: no —
   `brink opt` is explicit in v1, so an artifact's provenance is never
   ambiguous. Alternative: compile runs a default pass set, with
   `--no-opt` to suppress.
2. **Is the optimized artifact distinguishable?** Proposal: a flag in the
   artifact header recording that it was optimized and by which passes,
   so a bug report can say. Alternative: nothing, and the artifact is
   opaque.
3. **Per-pass toggles now or later?** Proposal: `--passes=` from the
   start, because a post-compile tool makes bisection cheap and it costs
   nothing to expose. This is the one place the framework document's
   deferral could reasonably be pre-empted.
