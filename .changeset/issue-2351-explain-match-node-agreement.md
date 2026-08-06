---
"@brink-lang/web": patch
---

Issue #2351: `explainMatch`/`explainMatchDoc` now agree with the compiler
on cue, parenthetical, compact-cue, and slugged/tag-bearing scene-heading
lines — previously they misreported exactly these four shapes.

`brink_ir::hir::classify::classify_line` matched a preset's pattern
against a line's *whole raw text*, while the compiler's own claiming
path (`hir::lower_native::element::candidate`/`try_claim`) matches
against a *sub-node's* extracted text (a cue's `CUE_NAME` alone, a
parenthetical's inner `TEXT` alone, a compact cue's `CUE_NAME` segment,
a scene heading's `SCENE_TITLE` stripped of its slug/tags). The two
matchers structurally could not agree: a real `@VENDOR` cue line the
compiler claimed reported a flat `matched: false` from `explainMatch`,
because the `cue` handler's own pattern (with no `@` in its character
class) can never match the whole `"@VENDOR"` text.

`explain_match_impl` (`crates/brink-web/src/editor/explain_match.rs`)
now finds the claim-candidate CST node under the cursor (via the new
`brink_ir::nearest_element_candidate`) and classifies that node's exact
sub-node text — the same input `try_claim` uses — instead of the raw
line, whenever the file's native parse tree has one for this line.
Falls back to the pre-#2351 raw-text walk unchanged for anything else
(an ink-dialect file, or a line outside the five claim-candidate
shapes). `ExplainMatchCache::explain` gained a new `node: Option<&SyntaxNode>`
parameter for this — a node-derived classification is never inserted
into the cache's raw-text memoization map (a chain-gated shape like
`PARENTHETICAL` can select a different sub-node for byte-identical text
depending on parser context alone, so caching it under the bare-text
key would be unsound); it still reuses the cache's already-compiled
pattern set.

As a consequence, `winner.kind` (issue #2310) can now finally report
`"cue"` and `"parenthetical"` for real `@NAME`/`(delivery)` lines — its
own composition logic was already correct, it was simply never reached
because the live walk missed those lines outright before this fix.
`"bang_dispatch"` is still not reachable (a `!name`-dispatched handler
is registered on a path this walk never consults at all) — tracked
separately.

Review follow-up: the claim-candidate node lookup now probes from the
line's own first non-whitespace byte, not the caret's raw offset — three
of the five claim-candidate shapes fuse their own trailing content into a
child `CONTENT_LINE` (an indented cue's surrounding whitespace/newline
sit outside the `CUE` node, a compact cue's literal dialogue and a
`!name` bang-dispatch's remainder are both fused `CONTENT_LINE` children),
so the previous caret-offset probe could return the wrong node — or a
false claim for a bang-dispatch line the compiler never makes — purely
because of which column the cursor happened to sit on. The answer no
longer depends on caret column within the line.
