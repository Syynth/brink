---
"@brink-lang/web": patch
---

Compiler: inline conditional/sequence branches now recognize as `Plain`/`Template` line-table entries instead of always falling back to fragmented `EmitContent` (issue #1667, the 2026-03-15 decision-log ruling).

`hir::normalize_file` already lifted an inline conditional/sequence out of its content line and spliced the surrounding prefix/suffix text into each branch (added the same day as the ruling), giving each branch its own child container that reaches the ordinary content recognizer independently — but the spliced `Text` parts were never merged, so the recognizer's `Plain` pattern (exactly one `Text` part) could never match. Every branch with an inline conditional/sequence silently fell back to `EmitContent`, which still emits one line-table entry per fragment — the exact "runtime assembles text from parts, translators see shredded fragments" shape the ruling was meant to retire.

`normalize.rs::extend_merging_text` merges adjacent `Text` parts at each splice seam, collapsing doubled whitespace the same way the runtime's own `Spring` word-break deduplication already did at read time — so rendered output is unchanged, but a branch like `{x: sunny|rainy}` in `"It was {x: sunny|rainy} today."` now compiles to two independent `RecognizedLine::Plain` line-table entries ("It was sunny today." / "It was rainy today.") instead of three fragmented `EmitContent` entries per branch.

`source_hash` impact: a branch that reaches `Plain`/`Template` now gets one clean `source_hash` over its full composed text, instead of several fragment-level hashes under `EmitContent`. Any `.xlf` translation unit exported from the old fragmented line table is orphaned by this change — a real translation-memory migration question for any story with inline conditionals/sequences that already has translated content, not something this fix absorbs silently.

Known gap, not fixed here: inline conditionals/sequences embedded directly in a choice's own display/bracket/inner text (`* Pick {x: A|B}`) are untouched — `normalize_file` never walks choice display text, only choice bodies — and still assemble from parts at runtime. Filed as a follow-up on issue #1667.
