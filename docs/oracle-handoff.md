# Oracle Comparison — Handoff Notes

## What we built

A C# oracle harness (`tools/ink-oracle/`) that compiles `.ink` source with inklecate, runs the reference C# ink runtime, and DFS-explores all branches. It emits per-`Continue()` oracle episodes (`.oracle.json`) as golden files. On the Rust side, the brink explorer now uses `continue_single_observed()` (not `continue_maximally`) and the oracle comparison test (`oracle_comparison.rs`) diffs brink's output against the oracle.

## Current state

- **318/385 cases pass, 3,518/6,561 episodes match** (ratchet at 3,500)
- 26 cases skipped (compile errors, empty source)
- 1,758 episodes "missing" — oracle branches brink doesn't produce (different choice availability)
- Stash has an incomplete attempt at terminal step splitting + output buffer commit (see below)

## The step alignment problem (open)

The oracle and brink have different step models for terminal events:

**Oracle (C# `Continue()`):** The last `Continue()` call that produces text has its outcome overwritten to Choices/Done/Ended by `SetLastStepOutcome`. So text and terminal are in the same step: `{ text: "Hello\n", outcome: Choices }`.

**Brink (`continue_single`):** Returns `Line::Text` for intermediate output and `Line::Choices`/`Line::Done`/`Line::End` for terminal events. The terminal `Line` variant has its own `text` field, so text + terminal can be in one step. But the text is sometimes split across a `Line::Text` + `Line::Choices` pair.

We tried two approaches:

1. **Separate terminal steps in the oracle** (option 1): Change the oracle to emit terminals as their own step with empty text. This aligns with brink's model when brink splits text from terminal. But it caused regressions (318→256) because brink sometimes combines text+terminal in one `Line::Done`/`Line::End`, and the oracle now always splits them. Edge case: the C# runtime sometimes does two `Continue()` calls where brink does one (empty text + terminal).

2. **Output buffer commit mechanism**: At yield points (done/choices/ended), mark trailing newlines as "committed" so they yield as separate steps. This is needed for the sequence newline issue but interacted badly with approach 1 because it changed step counts.

**Next steps:** These two problems need to be solved together. The cleanest path is probably:
- Have the oracle stamp terminals onto the last step (current behavior) — this matches how brink's `Line::Choices { text, .. }` works
- Fix the **newline issue** separately via the output buffer commit mechanism
- The newline after choosing (step 1 in `stopping`) is a separate `Continue()` call — brink needs to emit it as a separate `Line::Text { "\n" }`

## Remaining failure categories (40 cases)

From sampling the mismatches:

| Pattern | ~Cases | Example |
|---------|--------|---------|
| Missing `\n` after choice/sequence re-entry | 9 | `stopping`, `cycle`, `once` — newline between turns not yielding |
| Missing newline between logic lines | 3 | `I097`, `I037` — `"text1\ntext 2"` vs `"text1text 2"` |
| Function return values not appearing | 3 | `func-inline`, `I117-factorial` — `"4.4"` vs `""` |
| External function fallback differences | 6 | brink produces fallback output, oracle doesn't |
| Missing interpolation in choice text | 2 | `logic-in-choices` — `"Hello Joe"` vs `"Hello "` |
| Extra steps in brink | 4 | `divert-choice`, `varying-choice` — brink produces output oracle doesn't |
| Missing number prefix in print_num | 2 | `"one hundred"` vs `"hundred"` |
| Sequence off-by-one | covered by newline issue | |

## Key files

- `tools/ink-oracle/` — C# oracle CLI
- `crates/internal/brink-test-harness/src/oracle.rs` — Rust oracle types + comparison
- `crates/internal/brink-test-harness/tests/oracle_comparison.rs` — Primary test
- `crates/internal/brink-test-harness/src/explorer.rs` — DFS explorer (per-`continue_single`)
- `crates/internal/brink-test-harness/src/runner.rs` — Episode recorder (per-`continue_single`)
- `crates/brink-runtime/src/output.rs` — Output buffer (`has_completed_line`, `take_first_line`, `resolve_parts`)
- `crates/brink-runtime/src/story.rs` — `step_single_line` yield logic
- `crates/internal/brink-ir/src/hir/lower.rs` — HIR choice lowering (`replace_trailing_ws_with_spring`, `emit_content_line_stmts`)

## Key commands

```sh
# Run oracle comparison (primary test)
cargo test -p brink-test-harness --test oracle_comparison -- --nocapture

# Single case
BRINK_CASE=I002 cargo test -p brink-test-harness --test oracle_comparison -- --nocapture

# Corpus report (per-category breakdown)
cargo test -p brink-test-harness --test corpus_report -- --nocapture

# Regenerate oracle golden files
cd tools/ink-oracle && dotnet run -- --crawl ../../tests/ --force

# Compile and inspect bytecode
cargo run -p brink-cli -- compile /path/to/story.ink
```

## The C# runtime's Continue() loop (important context)

From `Story.cs` lines 462-481:
```csharp
do {
    outputStreamEndsInNewline = ContinueSingleStep();
    if (outputStreamEndsInNewline) break;
} while(canContinue);
```

`Continue()` breaks when either: (a) the output stream ends in a newline, or (b) `canContinue` is false. This is why each `Continue()` returns one line of text — newlines are natural yield points. Brink's `step_single_line` has the same concept via `has_completed_line()` / `take_first_line()`.

The difference: brink requires content AFTER a newline to consider it "committed" (for glue safety). The C# runtime doesn't — it just checks if the output stream ends in `\n`. This is why the `stopping` case fails: the newline between the sequence and the choice is the last thing before the yield point, so brink won't commit it.
