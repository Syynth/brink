# The optimizer framework — DEFERRED

**Status: STUB. Deliberately unwritten.**

This document will hold the general framework: the enumerated observable
surface and the per-pass contract against it. It is deferred until at
least two passes from `docs/optimizer-catalogue.md` have been
implemented and have pushed against real constraints.

That is a decision, not an omission. Writing it now would mean inventing
a contract from imagination and discovering later that it is the wrong
shape — and pass metadata is cheap to add while there is one pass and
expensive once there are five. `docs/optimizer-spec.md` is deliberately
built without any of it, so nothing here is prejudged.

The deferral has already paid once: this document previously asked
whether the optimizer was one stage or two, and the answer arrived from
working the catalogue rather than from reasoning — the optimizer is one
post-compile stage, and the transform that did not fit was never an
optimization at all (`docs/reachability-prune-spec.md`).

## What this document will have to answer

### 1. The observable surface, enumerated

The optimizer's real difficulty is that brink's observable surface is
larger than "the text a player sees". At least:

- output text and tags
- choice sets **and choice indices** (#3527 established indices are
  observable)
- visit counts and turn counts, per container
- save-state keys, which are definition paths
- line-table entries — the translation surface
- debugger addresses, breakpoints, and the bytecode-offset map
- effect rows under `--features effect-trace`
- artifact determinism itself

The list must be canonical and complete before any pass can honestly
declare what it preserves.

### 2. Is the effect trace contract or debug surface?

Forced by the catalogue's redundant-pure-work entry, which uses effect
rows as its evidence *and* changes them by succeeding.
`t2_ground_truth_effects` currently pins them.

### 3. Per-pass declarations

What a pass states about itself: which observables it preserves, which it
may perturb, what its eligibility analysis excludes. This is the field
set deliberately absent from `optimize()`'s `Pass` today.

### 4. Toggle grammar and the fence's shape

`docs/optimizer-spec.md` §9.3 proposes `--passes=` from the start, since
a post-compile tool makes bisection cheap. If that is ruled in early,
this document inherits the question of what the toggles do to the fence:
the A/B matrix must stay linear — each pass against the unoptimized
artifact, plus the default set — rather than 2^N.

### 5. Where a transform belongs

The prune moved out of the optimizer once the boundary was stated
(`docs/optimizer-spec.md` §7: the compiler decides what to ship, the
optimizer makes what ships cheaper). The line-table dedup entry raises
the same question in the other direction — it could be fixed in codegen
by interning entries at birth. This document should give a rule for
deciding, rather than settling each case by argument.
