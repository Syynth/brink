# Test Corpus

The repository includes a test corpus at `tests/` organized into tiers.

## Corpus structure

```text
tests/
  tier1/          # Basic ink features (text, choices, diverts, knots, variables)
  tier2/          # Intermediate features (tunnels, threads, lists, logic)
  tier3/          # Advanced features (complex weave, edge cases)
  tests_github/   # Real-world .ink files from open-source projects
  tests_patched/  # Modified tests for edge cases
```

## Test case format

Each test case is a directory containing:

| File | Description |
|------|-------------|
| `story.ink` | The ink source file (ground truth) |
| `story.ink.json` | Inklecate-compiled JSON output (kept only for oracle regeneration) |
| `episodes/*.episode.json` | Recorded play-throughs with expected output |

An episode records a sequence of continues and choice selections with the expected text output at each step. The test harness compiles the `.ink` source with the native compiler, replays each episode, and compares the output turn-by-turn against the recording.

## Running corpus tests

```sh
# Corpus report -- per-category pass/fail breakdown (run first for triage)
cargo test -p brink-test-harness --test corpus_report -- --nocapture

# All episodes (insta snapshots vs C# oracle)
cargo test -p brink-test-harness --test oracle_snapshots -- --nocapture

# Single case with diagnostics
BRINK_CASE=I002 cargo test -p brink-test-harness --test oracle_snapshots -- --nocapture

# Accept snapshot changes after intentional behavioral changes
INSTA_UPDATE=always cargo test -p brink-test-harness --test oracle_snapshots
```

Each case has a per-case snapshot in `crates/internal/brink-test-harness/tests/snapshots/`. Failing episodes are listed with step-by-step diffs against the oracle.

## The ratchet

`RATCHET_EPISODE_COUNT` in `oracle_snapshots.rs` is the minimum number of passing episodes. It only goes up — the test fails if the pass count drops below it. If a correct fix reveals previously-false passes, the ratchet can be lowered with an explanation.

## The inkjs sanction

The C# oracle needs `dotnet`, which cloud sessions do not have. `tools/inkjs-oracle` is the same crawler ported onto inkjs 2.4.0 (with .NET's `System.Random` installed in place of inkjs's own generator, so shuffles and `RANDOM` draw the reference's sequence). It never writes next to a golden — its job is to be *checked against* them:

```sh
cd tools/inkjs-oracle && npm ci && node --test && cd ../..
# every C# golden in the corpus, replayed through inkjs, must match
BRINK_INKJS_ORACLE=1 cargo test -p brink-test-harness --test inkjs_sanction -- --nocapture
BRINK_CASE=shuffle BRINK_INKJS_ORACLE=1 cargo test -p brink-test-harness --test inkjs_sanction -- --nocapture
# generated stories: brink vs inkjs (the nightly lane; PROPTEST_CASES raises the count)
BRINK_INKJS_ORACLE=1 cargo test -p brink-gen --test inkjs_differential -- --nocapture
```

The sanction compares the raw episode JSON after two normalisations (the C# tool's error-message wrapper and source paths; double- vs single-precision float printing — see `brink_test_harness::inkjs`'s header for the measurement behind each). `KNOWN_DIVERGENCES` in `inkjs_sanction.rs` lists any case where the two reference runtimes genuinely disagree, with a reason, checked both ways like `expected_mismatch`; it is empty. A one-off comparison of a single story is `node tools/inkjs-oracle/oracle.mjs path/to/story.ink --output-dir /tmp/out`, then `diff -r` against the case's `oracle/`.

## The capture tier: `tests/tier4-generated/`

Stories that came out of the generator (issue #3380, `docs/program-generator-spec.md` §5) — a shrunk proptest counterexample, or a hand-minimised probe against the reference — live in their own tier, one directory per case:

| File | Description |
|------|-------------|
| `story.ink` | The shrunk story |
| `oracle/*.oracle.json` | Golden episodes, same shape as the C# oracle's |
| `case.toml` | `[provenance]`: `source` (`proptest`/`probe`), `property`, optional `seed`, `oracle-source` (`inkjs`/`csharp`), optional `issue`; plus `[source] expected_mismatch` when the case pins a known open divergence |

The tier is **not** part of `RATCHET_EPISODE_COUNT` — the shared corpus walk prunes the directory, so nothing here reaches `oracle_snapshots`, the inkjs sanction, or the respell sweep. Its own must-pass target is `cargo test -p brink-test-harness --test tier4_generated`: every case matches its golden (or is flagged `expected_mismatch`, checked both ways), and `GENERATED_CASE_COUNT` there only moves through a promotion. `corpus_report` prints the tier in its own section.

Promote with the script, which refuses a story brink cannot compile or the oracle cannot golden, writes the case, and bumps the count:

```sh
pnpm promote:generated -- --name glue-space-after-interpolation --story shrunk.ink \
    --property inkjs_differential --issue "#3507"
# from a saved failing run of a brink-gen property (the `--- source ---` block)
pnpm promote:generated -- --name my-case --from-log run.log --property inkjs_differential --seed "cc …"
# a hand-minimised probe pinning an open bug
pnpm promote:generated -- --name empty-then-branch-else --story probe.ink --source probe \
    --property "probe: #3507 shapes against inkjs" --issue "#3510" --expected-mismatch "#3510"
# maintainer-local: re-bless an existing case with the C# oracle (dotnet) and flip oracle-source
pnpm promote:generated -- --name my-case --rebless-csharp
```

The golden comes from `tools/inkjs-oracle` (`npm ci` there first); only the C# oracle is on the trust hierarchy, so a case blessed by inkjs is evidence at the sanction's strength, and `--rebless-csharp` is how it graduates.

## GitHub corpus

The `tests_github/` directory contains real-world `.ink` files from open-source projects. These are used for parser smoke tests (zero panics on any input) and lossless roundtrip validation.

## Probe-found edge cases

Per maintainer directive (2026-09-02, `docs/decision-log.md`), a hand-minimized edge case discovered by the gen-expressions generator or by reference-differential probing against the C# ink runtime becomes a permanent corpus case, not a one-off fix. Cases added this way carry `origin = "brink"` in `metadata.toml`; a case documenting a *known* mismatch (brink diverges from the C# oracle) is still added with a real oracle golden and an `expected_mismatch` flag (see below) recording the gap. The first batch of these covers nested-gather fallback-choice semantics (#3383), multi-conditional lifting (#3386), sequence sharing across lifted branches (#3275), and lift-order of function-call side effects (#3395).

## `expected_mismatch`: pinning a known divergence mechanically

A case that is added *knowing* it currently mismatches the C# oracle — to lock in the oracle-correct golden as a permanent regression target while the underlying bug stays open — sets `expected_mismatch` in its `metadata.toml`'s `[source]` table, naming the tracking issue:

```toml
[source]
origin = "brink"
original_id = "lift-order-seq-fn-cond"
expected_mismatch = "#3395"
```

The harness (`brink_test_harness::corpus::expected_mismatch_issue` and `mismatch_flag_verdict`, issue #3402) reads this field in both `oracle_snapshots.rs` and `corpus_report.rs`, rather than the two trusting a hand-maintained doc-comment enumeration of which cases are the expected failures:

- An **unflagged** case that mismatches or is missing episodes fails exactly as it always has — the corpus summary snapshot changes, and `corpus_report` reports it under its category.
- A **flagged** case that still mismatches or is missing episodes is the expected, steady state — `corpus_report`'s "EXPECTED-MISMATCH CASES" section lists it (with its issue number) as "still mismatching, as expected", and its episodes are not required to clear `RATCHET_EPISODE_COUNT`.
- A **flagged** case whose episodes *all* now match the oracle — the underlying bug got fixed — makes `oracle_snapshots` fail outright, naming the case and its issue: the flag must be removed from `metadata.toml` and `RATCHET_EPISODE_COUNT` raised in the *same* change that fixed it, not left to drift silently. `corpus_report` marks the same case "⚠ NOW MATCHES THE ORACLE".

This means a fix landing for a flagged case can never leave the ratchet and the flag disagreeing about whether the case counts. The two cases that carried the flag first — `tests/tier2/evaluation/lift-order-seq-fn-cond` and `tests/tier2/evaluation/lift-order-fn-then-cond` (both #3395, the snippet above is the former's) — went through exactly that cycle: added as known mismatches, then flipped to passing by the #3395 fix (the lift-order hoist, 2026-09-04), which removed their flags and raised `RATCHET_EPISODE_COUNT` in the same change. No case carries the flag today. A `notes` field alongside `expected_mismatch` is still welcome for human-readable context, but only `expected_mismatch` is read by the harness.

A follow-up case, `tests/tier2/sequences/sequence-leads-multi-construct-line` (#3401), covers a stateful sequence *leading* a multi-construct line (sequence, then inline conditional, then a second sequence): across three views the oracle advances `apc`, `bpd`, `bpe`. It was added as a known mismatch (brink advanced `apc`, `bpc`, `bpd`) and flipped to passing with the #3401 fix, which is the intended life cycle of such a case — the golden was correct from day one, and the ratchet rose when brink caught up. The fix landed with two more cases pinning the shapes it had to get right: `sequence-cloned-into-glued-line` (`{a|b}{c|d|e} <>`, where trailing glue keeps the line off the variant path) and `sequence-shared-across-mixed-claim-branches` (one lifted branch claims as a variant line, the other cannot, and both must advance one counter).
