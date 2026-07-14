# Conformance

## Two kinds of "correct"

brink's compiler and runtime are checked against real ink two different
ways, and it matters which one a given piece of content is subject to:

- **Vanilla ink** — everything this chapter *doesn't* describe — is
  correctness-checked against a **C# ink oracle**: thousands of golden
  transcripts produced by the reference `inklecate`/ink-engine
  implementation, diffed episode-for-episode against what brink produces.
  This is what "the ratchet" means elsewhere in this project's tooling: a
  running count of oracle episodes that currently match byte-for-byte.
- **The brink dialect** has **no such oracle** — there has never been a
  reference ink implementation with multi-line `~ { … }` blocks, sigil
  collection literals, or postfix indexing, so there's nothing for the
  reference toolchain to generate golden transcripts from. Dialect
  correctness is instead checked against **hand-derived expected output**,
  written straight from the ruled semantics in `docs/t1b-surface-spec.md` —
  a separate corpus (`tests/tier1-brink/`), exercised the same way but
  without an external authority to diff against.

Neither corpus is optional or secondary to the other; they check different
claims. The oracle corpus proves brink reproduces real ink. The tier-1 brink
corpus proves the dialect extensions do what their own spec says they do.

## Strict-ink keeps the oracle's meaning intact

This is why [strict-ink is the default](./enabling.md): the oracle
comparison is only meaningful for programs the reference implementation can
also run, and the reference implementation has never seen dialect syntax.
If dialect extensions could leak into oracle-anchored content silently, the
oracle count would stop meaning what it says it means.

So the compiler's own conformance testing enforces the boundary
mechanically, not by convention: **the entire oracle corpus compiles under
`strict-ink`.** Every `.ink` file with a golden C# transcript is required to
be plain ink — if dialect syntax ever appeared in one, or if `strict-ink`
ever started accepting extension constructs, that's a hard CI failure, not a
lint warning.

## The dialect is authoring-time only

The choice of dialect never reaches the runtime. It's an input to
*analysis* — `AnalysisOptions::dialect`, set by the CLI's `--dialect` flag or
an equivalent library call — consumed entirely inside the compiler pipeline.
Two consequences follow directly:

- **Compiled output carries no trace of it.** A `.inkb` file produced from a
  brink-dialect source and one produced from strict-ink source are
  indistinguishable bytecode to `brink-runtime`. There is no dialect flag,
  version marker, or feature bit in the format for the runtime to consult.
- **The runtime has no dialect concept at all.** Loading and executing a
  story never depends on which dialect compiled it — by the time bytecode
  exists, "which surface syntax produced this" is a question that has
  already been answered and discarded.

This mirrors an existing precedent elsewhere in the toolchain (the
dialogue-dialect authoring convention used by the editor): a project-level,
authoring/tooling-time setting that shapes what the compiler accepts, but
that the shipped, running story never has to know existed.

## What this means for an author

If you're writing plain ink and never intend to use blocks, sigils, or the
stdlib, nothing in this chapter changes your workflow — `strict-ink` is
already what you're compiling under, and the oracle-anchored guarantees
apply to your content in full.

If you do want the dialect, turning it on is a project-wide, visible
decision (see [Enabling the Dialect](./enabling.md)) — and from that point,
the dialect-extension parts of your story are checked against the tier-1
brink corpus's semantics, not the C# oracle, because there is nothing else
to check them against. That's not a lesser guarantee, just a different one:
it's "matches the spec's ruled behavior" rather than "matches what real ink
does," because for this surface, real ink has no opinion.
