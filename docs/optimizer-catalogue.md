# Optimization catalogue

**Status: OPEN — this list grows.** Nothing here is committed to; an
entry is a candidate until its own document is ruled.

This is the list of what we intend the optimizer
(`docs/optimizer-spec.md`) to do, and it is the artifact the rest
derives from:

- the **generator knobs** exist to emit each entry's input shape
  (`docs/program-generator-spec.md`), so a pass without an entry here has
  no way to be property-tested;
- the **metrics** exist to show each entry's target moved
  (`docs/optimizer-spec.md` §3);
- the **observable-surface contract**, when it is written
  (`docs/optimizer-framework-spec.md`), is the union of what these
  entries collide with.

Writing the catalogue before the framework is deliberate. The
alternative — guessing at generic "bait" shapes and hoping they match
some future pass — gets the dependency backwards.

**Not in this catalogue:** anything the *compiler* does. Reachability
pruning is the compiler deciding what to ship, not the optimizer making
what ships cheaper (`docs/reachability-prune-spec.md`) — and so are
constant folding and dead-branch elimination, which need types,
provenance and lowering's invariants that no artifact carries. Those
transforms live in the compiler's own LIR layer, which is what the
2026-08-06 ruling's `LIR → passes → LIR` mechanism was for. This
catalogue is the artifact-level list only.

## The per-pass document template

Every entry graduates into a document with these sections, in order:

1. **Behavior** — what it does, precisely enough to implement.
2. **Constraints** — every observable it collides with, and the
   eligibility analysis that keeps it legal. For most passes this is the
   *bulk* of the work: the rewrite is usually trivial once you know what
   you are allowed to touch.
3. **Generator** — the knob that emits its input shape, and the property
   it makes possible. The standard pair: *traces agree between the
   optimized and unoptimized artifact*, **and** *the pass's metric
   actually moved*. The second half catches a pass that silently does
   nothing.
4. **Questions for the ruling.**

## The catalogue

| Pass | Target metric | Collides with | Status |
|---|---|---|---|
| dedup line table | **translatable units** | VO slots, source hashes, translator context, debug anchors | **done in codegen, not as a pass** (2026-09-05) — `docs/intl-spec.md` §"Line-table deduplication" |
| peephole | bytecode bytes, **opcodes executed** | choice indices, effect rows, debug offsets | **landed** — `docs/optimizer-peephole.md` (engine + `emit-line-nl` + `binary-fusion` + `left-operand-fold`, 2026-09-05) |
| literal-pool / name-table compaction | pool sizes, artifact bytes | id stability, save-state keys | candidate |
| eliminate redundant pure work | bytecode bytes | effect rows (which are also the proof) | candidate — blocked on a ruling |
| inline containers | containers, bytecode bytes | visit counts, save keys, debug addresses, line-table anchors | candidate — probably not worth it |

### dedup line table

**Resolved 2026-09-05 — in emission, not here.** The entry's own first
question ("born deduplicated, or deduplicated after the fact?") was ruled
for codegen: `brink-codegen-inkb` now consults a per-scope
`(content, slot_info)` index in `push_line`, so a line authored twice in
one scope is one translation unit from the moment it is emitted, on every
compile road including the studio's. The rulings on the three blocking
fields are in `docs/intl-spec.md` §"Line-table deduplication" and the
decision log. What follows is the original candidate reasoning, kept for
the record.

Glued cues and other repeated content produce repeated line-table
entries, so the same string reaches a translator more than once. Merging
identical units removes work a translator is currently paying for twice.

**Its metric is the only one denominated in human cost rather than
machine cost**, which makes it the most valuable entry here.

The duplication is not a property of the table — it is how entries are
born. In `brink-codegen-inkb`, `intern_string` consults a lookup map
while `add_line` / `add_template_line` append one row per occurrence site
with no lookup at all. The two sit next to each other in the same
emitter.

That has a consequence worth stating: **this could be fixed in codegen
instead**, by giving line entries the interning their neighbours already
have. Whether it belongs there (born deduplicated) or here (deduplicated
after the fact, on artifacts the compiler never saw) is this entry's
first question. Doing it in the optimizer keeps it toggleable and applies
it to existing artifacts; doing it in codegen is smaller and earlier.

Three fields of `LineEntry` block a naive text-keyed merge:

- **`audio_ref`** — identical text with different VO takes. Merging
  collapses two distinct VO slots.
- **`source_hash`** — per-unit staleness detection. A merged unit can no
  longer say *which* source changed.
- **`source_location`** — the debugger and explain-match anchor. A merged
  entry is one-to-many and needs a provenance list.

Plus the standing i18n trap: identical source text frequently needs
*different* translations by context, so the key is at most
`(content, audio_ref, …)` with a per-unit opt-out.

And a scope question: `scope_line_table` is **per-scope**, so
deduplicating within a scope is the conservative reading. Going global
merges more units but merges them across contexts — which is the
translator-context concern again, at a larger radius.

### peephole

**Graduated: `docs/optimizer-peephole.md`.** The shared rewriting engine
(labels, relocation of jumps/addresses/debug entries) and the first
superinstruction, `EmitLine → EmitNewline` ⇒ `EmitLineNl` (`.inkb` v7),
landed 2026-09-05. The three collisions below are each resolved there; what
follows is the original candidate reasoning, kept for the record.

The reason the optimizer is post-compile at all: bytecode does not exist
until codegen, so this can live nowhere else.

Constraints are the ones the runtime can observe from instruction
sequences rather than from definitions: **choice indices** (#3527
established these are observable), **effect rows** if they are contract
rather than debug surface, and **debug offsets**, since `DebugInfo` maps
bytecode offsets to source and any instruction removed shifts the ones
after it.

That last one is the practical difficulty: a peephole that rewrites
instructions must rewrite the debug map with them or drop it.

### literal-pool / name-table compaction

Unreferenced pool entries after other passes have run — and after the
compiler's prune, which deliberately leaves name-table slots unused
rather than renumbering. Collides with id stability: compaction *is*
renumbering, so anything holding an id across the boundary (save-state
keys above all) has to be considered.

### eliminate redundant pure work

Effect rows record what a chunk reads and writes, so they are the
evidence that two evaluations are redundant — and `StoryData` ships
`effect_rows`, so the evidence is right there in the artifact.

They are *also* observable under `--features effect-trace`, with
`t2_ground_truth_effects` pinning them, and eliminating work changes the
trace. So this entry cannot be specified without ruling: **is the effect
trace part of the behavioral contract, or a debug surface an optimizer
may perturb?** It is the sharpest deferred framework question, and this
pass is what forces it.

### inline containers

Listed for completeness and scepticism. The transformation is trivial;
the eligibility analysis is the whole pass. A container may not be
inlined when anything reads its visit or turn count (`{knot}`,
`TURNS_SINCE`), when a save can name it, when it carries line-table
entries the intl pipeline exports, when it is author-labelled, or when a
debugger address anchors to it.

What survives those exclusions is probably small. Measure before
committing.

## Surface neutrality

Every entry above operates on artifacts, so **the optimizer serves ink
and native equally**. This was not true of the earlier design: when the
optimizer was a LIR stage whose only pass was the mount prune, it did
nothing for ink at all, permanently and by construction.

Moving it post-compile dissolved that asymmetry rather than papering
over it. The one native-only transform — the prune — is now correctly
filed as a compiler step, where being native-only is unremarkable
because it is a fact about the mount rather than about optimization.
