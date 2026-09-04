# Optimization catalogue

**Status: OPEN — this list grows.** Nothing here is committed to; an
entry is a candidate until its own document is ruled.

This is the list of what we intend to optimize, and it is the artifact
the rest of the optimizer derives from:

- the **generator knobs** exist to emit each entry's input shape
  (`docs/program-generator-spec.md`), so a pass without an entry here has
  no way to be property-tested;
- the **metrics** exist to show each entry's target moved
  (`docs/lir-optimizer-spec.md` §5);
- the **observable-surface contract**, when it is written
  (`docs/optimizer-framework-spec.md`), is the union of what these
  entries collide with.

Writing the catalogue first is deliberate. The alternative — guessing at
generic "bait" shapes and hoping they match some future pass — was the
earlier plan, and it gets the dependency backwards.

## The per-pass document template

Every entry graduates into a document with these sections, in this
order. `docs/optimizer-pass-prune.md` is the worked example.

1. **Behavior** — what it does, precisely enough to implement.
2. **Constraints** — every observable it collides with, and the
   eligibility analysis that keeps it legal. For most passes this is the
   *bulk* of the work: the rewrite is usually trivial once you know what
   you are allowed to touch.
3. **Generator** — the knob that emits its input shape, and the property
   it makes possible. The standard pair: *traces agree between
   `OptLevel::None` and `Default`*, **and** *the pass's metric actually
   moved*. The second half is what catches a pass that silently does
   nothing.
4. **Questions for the ruling.**

## The catalogue

| Pass | Target metric | Collides with | Can run on the LIR seam? | Status |
|---|---|---|---|---|
| prune unreachable | containers, name table | nothing (nothing reachable is removed) | yes | `optimizer-pass-prune.md`, needs ruling |
| dedup line table | **translatable units** | VO slots, source hashes, translator context, debug anchors | **no — see below** | candidate |
| inline containers | containers, instructions | visit counts, save keys, debugger addresses, line-table anchors | yes | candidate |
| eliminate redundant pure work | instructions | effect rows (which are also the proof) | yes | candidate |

### prune unreachable

Native-only: an ink project never carries the mount. Fully specified in
its own document; four open questions.

### dedup line table

Glued cues and other repeated content produce repeated line-table
entries, so the same string reaches a translator more than once. Merging
identical units does not lose any — it removes work a translator is
currently paying for twice — which is why this pass is worth calling out
against the prune's "never lose a translatable unit" constraint. The two
are compatible: that constraint forbids *losing* units, not *merging*
them.

**Its metric is the only one in the system denominated in human cost**
rather than bytes, which makes it the most valuable entry here and the
hardest to evaluate from a size report alone.

Three fields of `brink_format::LineEntry` block a naive text-keyed merge:

- **`audio_ref`** — identical text with different VO takes. Merging
  collapses two distinct VO slots.
- **`source_hash`** — per-unit staleness detection. A merged unit can no
  longer say *which* source changed.
- **`source_location`** — the debugger and explain-match anchor. A merged
  entry is one-to-many and needs a provenance list, not a single site.

And the standing i18n trap: identical source text frequently needs
*different* translations by context. Deduplicating by text takes that
choice away from the translator, so the key is at most
`(content, audio_ref, …)` with an explicit per-unit opt-out.

**Where it can run is an open problem.** `lir::Program` has no line
table — codegen builds those while emitting `StoryData`
(`docs/lir-optimizer-spec.md` §2). So this is not a LIR pass as the
stage currently defines one. Three shapes, none chosen:

1. a LIR pass that canonicalises *content* so codegen naturally emits
   fewer entries;
2. an interning step inside codegen, where the entries are born;
3. a post-codegen `StoryData` transform — which would mean the optimizer
   has two stages, not one.

That question belongs in this pass's own document, and it may send the
stage's definition back for revision. It is the clearest example of why
the framework document is deferred until real passes have pushed on it.

### inline containers

The transformation is trivial and the eligibility analysis is the whole
pass. A container may not be inlined when anything reads its visit or
turn count (`{knot}`, `TURNS_SINCE`), when a save can name it, when it
carries line-table entries the intl pipeline exports, when it is an
author-labelled target, or when a debugger address anchors to it. What
remains after those exclusions is worth measuring before committing to
the pass at all.

### eliminate redundant pure work

Effect rows record what a chunk reads and writes, so they are the
evidence that two evaluations are redundant — the analysis this pass
needs already exists in some form. They are *also* observable under
`--features effect-trace`, with `t2_ground_truth_effects` pinning them.
Eliminating work changes the trace.

So this entry cannot be specified without first ruling: **is the effect
trace part of the behavioral contract, or a debug surface an optimizer
may perturb?** That is the sharpest of the deferred framework questions
and this pass is what forces it.

## Not yet catalogued: anything ink-facing

Every entry above is native-facing or surface-neutral. With ink a peer
surface, the optimizer as catalogued does nothing for an ink program:
the prune has no mount to remove, and the one lever that would touch an
ink artifact — dropping unreachable author content — is refused on
translation-surface grounds (`optimizer-pass-prune.md` §2.1).

That is a real gap, stated rather than hidden. Filling it means passes
whose targets are ink-shaped — choice-table compaction, constant folding
of author expressions, output-fragment coalescing — each of which needs
its own observable-behavior argument against the ratchet. That is a
larger conversation than #2336 and wants its own ruling.
