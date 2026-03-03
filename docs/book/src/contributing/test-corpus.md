# Test Corpus

The repository includes a test corpus at `tests/` organized into tiers that match the implementation roadmap.

## Corpus structure

```
tests/
├── tier1/          # Basic ink features
│   ├── basics/
│   ├── basictext/
│   ├── choices/
│   ├── divert/
│   ├── diverts/
│   ├── gather/
│   ├── glue/
│   ├── knot/
│   ├── knots/
│   ├── stitch/
│   ├── variables/
│   └── weave/
├── tier2/          # Intermediate features
├── tier3/          # Advanced features
├── tests_github/   # Real-world .ink files from open-source projects
└── tests_patched/  # Modified tests for edge cases
```

## Test case format

Each test case is a directory containing:

<!-- TODO: document the test case format:
  - story.ink — the ink source file
  - story.ink.json — inklecate-compiled JSON output
  - expected_transcript — the expected output from running the story
  - input.txt (optional) — choice inputs for interactive stories
-->

## Running corpus tests

```sh
# Run all tier 1 corpus tests
cargo test -p brink-runtime corpus_tier1 -- --nocapture

# Run a specific test by name
cargo test -p brink-runtime corpus_tier1 -- I084 --nocapture
```

## How corpus tests work

<!-- TODO: explain the corpus test harness:
  - Loads .ink.json via brink-converter → .inkb
  - Runs the story through brink-runtime
  - Compares actual output against expected transcript
  - Reports diffs on failure
-->

## GitHub corpus

The `tests_github/` directory contains 1,115 `.ink` files and 937 golden `.ink.json` files from open-source projects. These are used for parser smoke tests (zero panics), lossless roundtrip validation, and future conformance testing.

## Adding new test cases

<!-- TODO: explain how to add new tests:
  - Naming convention (e.g., I084-sticky-choices-stay-sticky)
  - How to generate the expected transcript
  - When to use tests_patched vs a new test in the appropriate tier
-->
