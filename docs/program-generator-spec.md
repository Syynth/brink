# Story-level program generator — `brink-gen`

Status: **RULED 2026-09-02** (architecture, reference harness, capture
tier, crate, feature order; `docs/decision-log.md` "Program generator:
typed model + corpus mutation, inkjs as the reference harness, a
capture tier"). Serves every property in
`docs/observable-semantics-spec.md` §4.1 and the auto-fix `Safe`
obligation (`docs/autofix-spec.md` §3). Epic #3370; first sub-issues
#3378 (model + printer + structure tier), #3379 (inkjs harness),
#3380 (capture tier).

## 1. Why this exists

The corpus covers the shapes someone already wrote down. An optimizer's
bugs — and a fixer's, and a formatter's — live in the shapes nobody
did. The generator manufactures those shapes on demand, shrinks a
failure to a readable story, and (via §5) turns each one into a
permanent corpus case. **`.ink` is the first surface** (RULED
2026-09-01: ink support is the more urgent need); native follows
through the same model.

## 2. Architecture — RULED: A + C

**A — typed model, printed to source.** The generator builds a
*semantic* model first — a symbol table (knots, stitches, labels,
`VAR`s, `LIST`s, functions, externals) — and then bodies that reference
only declared names. Programs compile by construction; rejects are
rare and treated as generator bugs, not filtered noise. proptest
strategies range over the model, so **shrinking operates on the model**
(drop a knot, simplify an expression, remove a choice) and re-prints —
a counterexample arrives as a ten-line story a human can read. The
printer is the dialect switch: one model, an `.ink` printer now, a
`.brink` printer later (§7).

**C — corpus mutation.** Real stories from the corpus with small
semantic mutations applied (the mutator #3376 builds for the oracle's
sensitivity study). Cheap and realistic; coverage bounded by the
corpus, which is why it is the second source, not the first.

**Not B.** Extending the string-level strategies in
`brink-syntax/tests/proptest_syntax.rs` and filtering by compilation
was considered and rejected: rejects dominate, shrinking mangles text,
and it cannot target semantics. Those strategies stay what they are —
parser round-trip fodder — and lend their leaf generators
(`arb_ident`, `arb_text`) to the model where they fit.

## 3. Validity and termination by construction

- **Declare before use**, for every name kind. Temps are used only
  after their declaration on every path — the generator models
  dominance — except under a bait flag (§4) that deliberately violates
  it to feed E193's tests.
- **Terminating**: a DAG of knots plus bounded back-edges, each guarded
  by a once-only `*` choice or a visit-count condition; every flow ends
  in a divert, `END` or `DONE`. The explorer's step limit
  (`ExploreConfig`, `Story::STEP_LIMIT`) is the backstop, never the
  plan.
- **Ink-valid vs brink-superset**: the model knows which constructs
  the official compiler accepts (the compat-deny tier, #3373, is that
  vocabulary). The `plain_ink` profile emits only ink-valid programs —
  the ones the reference harness (§6) can judge; other profiles may
  emit the superset.
- **Externals**: the model declares `EXTERNAL` functions; a *run*
  carries a deterministic result table, served by the explorer's stub
  facility (added with #3376's full-trace work, since external calls
  are part of the trace).

## 4. Profiles — biasing as data

A uniform grammar walk almost never produces the case an optimizer
cares about. A `Profile` is a **data** struct, not a code path: weights
per construct, size bounds, and *bait* flags — dead stores, unused
temps, constant conditions, unreachable-after-divert, redundant draws,
shadowing, read-before-declaration. Each property picks its profile:
the optimizer wants bait; the reference differential wants
`plain_ink`; an auto-fix `Safe` fixture wants exactly the shape its
fixer discharges. Profiles ship as named constants; a property names
the one it uses in its own definition.

## 5. Capture tier — RULED

Interesting generated stories **join the corpus** as
`tests/tier4-generated/`: one directory per case holding the shrunk
story, its golden trace, and a `case.toml` with provenance — `source`
(the property and seed that produced it), `oracle-source` (`inkjs` or
`csharp`). Two admission routes:

1. **Counterexample capture**: a property failure's shrunk story is
   promoted by a script (`pnpm promote:generated`) that obtains the
   golden from the reference harness and refuses if the story does not
   compile or the golden cannot be produced. `proptest-regressions`
   files are checked in as well, so a found seed is never lost.
2. **Coverage-novel capture**: a story that exercises a construct
   combination (or opcode pair in the compiled output) no corpus case
   does can be promoted although nothing failed. The signal is chosen
   in #3380 and its cost stated there.

The tier is self-contained like `tests/tier1-native/`: **not** part of
`RATCHET_EPISODE_COUNT`, with its own must-pass target whose count only
moves through an explicit promotion. A maintainer-local step may
re-bless a golden with the C# oracle and flip `oracle-source`.

## 6. The reference harness — RULED: inkjs

The differential `trace(brink(P)) == trace(reference(P))` for generated
`plain_ink` stories runs on **inkjs** (2.4.0: runtime plus a JS port of
the compiler, `inkjs/compiler`) — Node-only, so it runs in CI and cloud
sessions, "not on my laptop." `tools/inkjs-oracle` compiles a story
and replays the harness's runs, emitting the full trace of
`docs/observable-semantics-spec.md` §2; runtime warnings are recorded,
never fatal (attach `onError` — the C# oracle's fatal-escalation
artifact behind PR #3369's I010 regeneration must not be reproduced).

inkjs is not on the trust hierarchy (`CLAUDE.md`), so it is
**sanctioned, not assumed**: it must replay every checked-in C# oracle
episode across tier1–3 and match. That result makes it a trusted proxy
for the rank-2 reference; the C# runtime remains the tie-breaker
(maintainer-local, dotnet) whenever inkjs and brink disagree. The RNG
seed mapping between inkjs and the harness is proven before draws
enter the differential profile (#3379 states which).

## 7. Feature order, crate, lanes

- **Crate**: `crates/internal/brink-gen`, its own workspace member —
  fmt, respell, ide and harness tests consume the strategies without
  pulling the harness in. proptest is already a workspace dependency.
- **Order** (the corpus ladder, RULED as-is): structure — knots,
  stitches, diverts, choices, gathers, glue (#3378) → variables,
  expressions, conditionals → functions, tunnels, threads → lists →
  sequences, cycles, shuffles, `RANDOM` → externals, tags → the
  `.brink` printer and the respell route (`trace(P) = trace(respell(P))`
  as one more property).
- **Lanes**: per-PR runs use small case counts (smoke: everything
  compiles through both roads with identical bytes and explores to
  termination); nightly runs use large counts and carry the inkjs
  differential.

## 8. Not covered

- Native generation and the respell route are sequenced last (§7);
  the model is designed for them but nothing here specifies the
  `.brink` printer.
- The coverage signal for §5's second route is chosen in #3380, not
  here.
- Generating *projects* (multi-file INCLUDE trees, `brink.toml`
  variants) — a later tier; the model is single-compilation for now,
  which is the unit every property is stated against anyway.
