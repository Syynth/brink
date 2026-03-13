# Test Corpus

The repository includes a test corpus at `tests/` organized into tiers.

## Corpus structure

```
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
| `story.ink.json` | Inklecate-compiled JSON output (reference) |
| `episodes/*.episode.json` | Recorded play-throughs with expected output |

An episode records a sequence of continues and choice selections with the expected text output at each step. The test harness runs both pipelines (native compiler and converter) against each episode and compares results.

## Running corpus tests

```sh
# Corpus report -- per-category pass/fail breakdown (run first for triage)
cargo test -p brink-test-harness --test corpus_report -- --nocapture

# All episodes
cargo test -p brink-test-harness --test brink_native_episodes -- --nocapture

# Single case with diagnostics
BRINK_CASE=I002 cargo test -p brink-test-harness --test brink_native_episodes -- --nocapture
```

For each failure, the harness prints:
1. The `.ink` source (what the story says)
2. The compiler's `.inkt` dump (what brink produced)
3. The converter's `.inkt` dump (what the correct output looks like)

## The ratchet

`RATCHET_EPISODE_COUNT` in `brink_native_episodes.rs` is the minimum number of passing episodes. It only goes up -- the test fails if the pass count drops below it. If a correct fix reveals previously-false passes, the ratchet can be lowered with an explanation.

## GitHub corpus

The `tests_github/` directory contains real-world `.ink` files from open-source projects. These are used for parser smoke tests (zero panics on any input) and lossless roundtrip validation.
