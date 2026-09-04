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

**Status (2026-09-04, #3380 route 1 landed).** `tests/tier4-generated/`
exists with `scripts/promote-generated.mjs` (`pnpm promote:generated`:
`--story` or `--from-log` a failing run's `--- source ---` block; refuses
when brink cannot compile the story or the inkjs oracle cannot golden it;
writes `story.ink`, `oracle/`, `case.toml`; bumps `GENERATED_CASE_COUNT`;
`--rebless-csharp` runs `tools/ink-oracle` and flips `oracle-source`).
The must-pass target is `brink-test-harness`'s `tier4_generated.rs`, with
`[source] expected_mismatch` carrying the corpus's two-way discipline, and
`corpus_report` prints the tier in its own section. The shared corpus
walk prunes the directory by name (`corpus::GENERATED_TIER_DIR`). First
cases: the two #3507 shapes (passing), the #3508 choice-text shape and
the #3510 empty-then-branch shape (expected mismatches). Route 2
(coverage-novel capture) is filed separately with the proposed signal.

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

**Status (2026-09-04, #3379 landed).** `tools/inkjs-oracle` is a port
of `tools/ink-oracle`'s crawler (same DFS, same episode JSON) onto
`inkjs/compiler`, and the sanction is
`crates/internal/brink-test-harness/tests/inkjs_sanction.rs`: **414 of
414** oracle cases match — 400 byte for byte, 14 after two
normalisations that forgive presentational artefacts of the *reference*
(the C# tool's no-`onError` error wrapper and absolute source paths;
double-precision float printing, `0.6666666666666666` vs `0.6666667`),
documented and unit-tested in `brink_test_harness::inkjs`. Two facts
that sanction measured:

- **There is no RNG seed mapping; there is a replacement.** inkjs ships
  a Park–Miller generator where the C# runtime uses `new
  System.Random(seed)` (Knuth subtractive) for every shuffle, `RANDOM`
  and `LIST_RANDOM`. Same seeds, different sequences, every draw
  diverges. `tools/inkjs-oracle/dotnet-random.mjs` ports .NET's
  generator (the same port `brink-runtime`'s `rng.rs` carries) and
  installs it over inkjs's `engine/PRNG` export; only then do the
  shuffle and `LIST_RANDOM` goldens match. The bundled `inkjs/full`
  cannot be patched this way, which is why the tool imports the
  unbundled compiler module.
- **Warn-and-continue is the default** (§6's requirement), with
  `--strict-warnings` reproducing the C# tool's no-handler mode; no
  checked-in golden needed the strict mode to match.

The differential itself is
`crates/internal/brink-gen/tests/inkjs_differential.rs` over
`Profile::PLAIN_INK`, compared by the harness's `diff_oracle` (the
corpus ratchet's own comparison). Its first run found two brink
divergences from ink — whitespace between an interpolation and `<>`
dropped (#3507), and whitespace runs in choice text collapsed
(#3508) — carried as issue-keyed `KNOWN_DIVERGENCES` there until each
gets its C#-golden corpus case and fix. Lanes per §7: the sanction is
per PR (`ci.yml`, `inkjs-sanction`); the differential is nightly
(`inkjs-differential.yml`, 512 cases, advisory). Both are opt-in
locally behind `BRINK_INKJS_ORACLE=1` after `npm ci` in
`tools/inkjs-oracle`.

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

**Status (2026-09-04, functions tier landed).** The ladder stands at
structure (#3378) → variables, expressions, conditionals → **functions**:
`Story::functions` holds `=== function f(a, ref b) ===` definitions with
typed parameters, bodies of items (lines, assignments, temps, conditional
blocks, statement calls — no tail, so no divert or choice, as ink
requires), and an optional `~ return expr`. Calls are typed like any
expression (`Expr::Call` in expression position for value functions,
`Item::Call` as a `~ f(x)` statement for void ones); a `ref` argument is a
visible variable of the parameter's type. The call graph is a DAG by
construction — function `i` calls only functions `< i`, flow code calls
any — so every call terminates without a step-limit crutch. `Profile`
gains `max_functions` (2 in `DEFAULT`/`PLAIN_INK`, 0 in `STRUCTURE` and
`RESPELLABLE`) and `max_params` (2). The tier's first differential run
(300 stories) found five divergences, each filed with a one-line
reproduction and carried as a `KNOWN_DIVERGENCES` predicate in
`tests/inkjs_differential.rs`: #3519 (a function's leading newline is
kept when the line already has content), #3521 (the #3395 lift runs a
printing call in a lifted construct's condition before the line's
prefix — the "reverse shape" the ruling left owed; needs a ruling),
#3522 (function-end trim stops at glue), #3523 (a multi-line `- else:`
arm lacks inklecate's leading newline), #3524 (a multi-line function
output in an interpolation is one `Step::Line`). None is a generator
defect; all five surface only when a function *prints* — the shapes
the hand-written corpus covers thinly.

**Status (2026-09-04, tunnels-and-threads tier landed).** Every knot
carries a `FlowKind`: the entry is a plain knot; a **tunnel** knot is
entered only by `-> t ->` (`Item::TunnelCall`) and its weaves leave by
`->->` (`Exit::TunnelReturn`), `-> END`, or a divert within the tunnel
flows under the back-edge rules; a **thread** knot is entered only by
`<- t` (`Item::Thread`) from a plain knot's weave and leaves by
`-> DONE`, `-> END`, or a divert within the thread flows. Tunnel calls
from flow code and threads may name any tunnel; from inside tunnel knot
`i` only a tunnel knot `> i`, so tunnel calls form a DAG like function
calls do. Diverts never cross kinds — one flow table, each kind a
contiguous range the decoder resolves raw exits within. `Profile` gains
`max_tunnels` (2) and `max_threads` (1), both 0 in `STRUCTURE` and
`RESPELLABLE`. First differential run: #3527 (`Choice.index` counted an
invisible fallback merged ahead of the main flow's choices by a thread),
fixed with the tier.

**Status (2026-09-04, lists tier landed).** `Story::lists` holds
`LIST name = a, (b), c` declarations; each is a global of its own
`Ty::List(i)` whose values are subsets of its items, item names unique
across the story (`li{i}_{j}`, so no item can collide with a list, knot,
var or function name). Expressions: list literals `(a, b)`, single
items, `+`/`-` (union/difference, the right operand an item half the
time), `^`, `?`/`!?` → bool, `LIST_COUNT` → int, `LIST_MIN`/`LIST_MAX`/
`LIST_ALL`/`LIST_INVERT`; `+=`/`-=` write through a list target;
`==`/`!=` compare same-typed values; temps and function parameters may
be list-typed, `VAR`s stay int/bool/str. An empty list is reachable only
through operations (a literal is never `()`), which is where ink's
empty-list spelling gets exercised. The tier's differential runs found
five runtime defects, all fixed with it: #3531 (containment with an
empty operand), #3532 (list origins), #3533 (a blank line before a turn
boundary — plus its sibling #3534, tag-only lines) and #3536 (a
function ending in a value that renders empty). One is carried as a
predicate instead: #3535, glue reaching across a blank line, which
predates the tier and which an empty-list interpolation is simply the
first thing to hit often. `Profile` gains `max_lists` (1) and
`max_list_items` (4), 0 lists in `STRUCTURE`/`RESPELLABLE`; the respell
property treats `LIST` like `VAR` under #3517 (a host-readable global
respelled private).

## 8. Not covered

- Native generation and the respell route are sequenced last (§7);
  the model is designed for them but nothing here specifies the
  `.brink` printer.
- The coverage signal for §5's second route is chosen in #3380, not
  here.
- Generating *projects* (multi-file INCLUDE trees, `brink.toml`
  variants) — a later tier; the model is single-compilation for now,
  which is the unit every property is stated against anyway.
