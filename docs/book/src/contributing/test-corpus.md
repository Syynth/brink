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

## GitHub corpus

The `tests_github/` directory contains real-world `.ink` files from open-source projects. These are used for parser smoke tests (zero panics on any input) and lossless roundtrip validation.

## Probe-found edge cases

Per maintainer directive (2026-09-02, `docs/decision-log.md`), a hand-minimized edge case discovered by the gen-expressions generator or by reference-differential probing against the C# ink runtime becomes a permanent corpus case, not a one-off fix. Cases added this way carry `origin = "brink"` in `metadata.toml`; a case documenting a *known* mismatch (brink diverges from the C# oracle) is still added with a real oracle golden and a `notes` field explaining the gap, and is called out separately so it is excluded from `RATCHET_EPISODE_COUNT` until fixed. The first batch of these covers nested-gather fallback-choice semantics (#3383), multi-conditional lifting (#3386), sequence sharing across lifted branches (#3275), and lift-order of function-call side effects (#3395).
