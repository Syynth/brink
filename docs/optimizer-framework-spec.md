# The optimizer framework — DEFERRED

**Status: STUB. Deliberately unwritten.**

This document will hold the general framework: the enumerated observable
surface, the per-pass contract against it, and the toggle grammar. It is
deferred until at least two passes from `docs/optimizer-catalogue.md`
have been implemented and have pushed against real constraints.

That is a decision, not an omission. Writing it now would mean inventing
a contract from imagination and then discovering it is the wrong shape —
and a pass metadata scheme is cheap to add while there is one pass and
expensive once there are five. The stage
(`docs/lir-optimizer-spec.md` §3.1) is deliberately built without any of
it, so nothing here is prejudged.

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
- debugger addresses and breakpoints
- effect rows under `--features effect-trace`
- `.inkb` determinism itself

The list needs to be canonical and complete before any pass can honestly
declare what it preserves.

### 2. Is the effect trace contract or debug surface?

Forced by the catalogue's "eliminate redundant pure work" entry, which
uses effect rows as its evidence *and* changes them by succeeding.
`t2_ground_truth_effects` currently pins them.

### 3. Per-pass declarations

What a `Pass` states about itself: which observables it preserves, which
it may perturb, what its eligibility analysis excludes. This is the
field set deliberately absent from the stage's `Pass { name, run }`.

### 4. Toggle grammar

A level dial plus per-pass override for bisection
(`--opt-disable=<pass>`), and what that does to the fence. The A/B matrix
must stay linear — each pass against baseline, plus the default set
against baseline, i.e. N+1 configurations rather than 2^N.

### 5. Whether the stage is still one stage

The catalogue's line-table dedup entry may not fit a LIR-only seam at
all. If it lands in codegen or post-codegen, "the optimizer" becomes two
stages and this document has to say how they relate.
