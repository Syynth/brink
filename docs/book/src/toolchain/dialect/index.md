# The Brink Dialect

Vanilla ink is a weave language: knots, stitches, choices, diverts, gathers,
and a thin logic layer (`~` lines, conditionals, `VAR`/`temp`) for steering
that weave. It has no arrays, no maps, no loops, no multi-line logic. Real
stories often want those things anyway — tracking an inventory, iterating a
quest list, building up a piece of interpolated text — and authors have been
faking them with lists and stringly-typed hacks since ink shipped.

The **brink dialect** is a superset of ink that adds them properly: multi-line
logic blocks, array/map literals, indexing, and a small mutating stdlib. It
sits entirely inside the existing `~` logic channel — narrative, choices, and
diverts are untouched — and it compiles to the same bytecode, run by the same
runtime, as everything else.

This chapter covers:

- **[Enabling the Dialect](./enabling.md)** — the `--dialect` flag, why
  `strict-ink` is the default, and what changes when you opt in.
- **[Logic Blocks](./blocks.md)** — `~ { … }`, the pure-logic fence, and what
  it deliberately can't do.
- **[Collections](./literals.md)** — `#[…]`/`#{…}` sigil literals, and why
  they only work in expression position.
- **[Indexing & Mutation](./indexing.md)** — `a[i]`, `a[i] = v`, and the
  runtime faults that replace ink's usual silent tolerance.
- **[Standard Library](./stdlib.md)** — `len`/`keys`/`values`/`contains` and
  the mutating `push`/`insert`/`remove`.
- **[Conformance](./conformance.md)** — how the dialect coexists with the
  oracle-anchored core, and what "authoring-time only" actually means.

## The shape of the design

Three decisions run through every page in this chapter, so it's worth naming
them up front:

1. **One grammar, two dialects.** `brink-syntax` always parses the full
   superset — the dialect extensions included — so the parser, IDE, and
   formatter never need to know which dialect a project has chosen. Whether a
   construct is *allowed* is decided later, during analysis.
2. **The extensions are pure logic.** Everything new is data manipulation
   inside `~` — no new narrative, choice, or flow-control surface. A block
   computes; it never weaves.
3. **Sigils, not new keywords.** Collection literals use `#[…]`/`#{…}`
   precisely because `#` cannot begin an ordinary ink expression — the new
   syntax is unambiguous with existing ink wherever it's legal to write.

The full ruling — including the alternatives that were considered and
rejected — lives in `docs/t1b-surface-spec.md`. This chapter is the
author-facing account of the same surface; when in doubt, that spec is the
tie-breaker.
