---
"@brink-lang/web": patch
---

HIR overlay: conditional/sequence arm prose now projects `content` spans
(issue #981).

`hir_projection::project_hir` walks into conditional/sequence branch
bodies correctly, but the ink-compat lowering (`brink-ir`'s
`hir::lower::block::{branch,branchless}`) always flushed a branch-body
`Content` node with `ptr: None` — those bodies have no per-line
`CONTENT_LINE` wrapper node the way a top-level line does, so `content.ptr`
was unconditionally absent. `ContentAccumulator` now tracks the covering
source range of the raw tokens (text/glue/escape/inline-logic) it buffers
for a branch body and stamps a synthetic-but-real-range `Provenance` on
flush — the same posture `conditional_with_expr::branchless_first_arm_span`
already uses for a branch's own span: it never resolves back to a live
syntax node, but carries an exact byte range for span-consuming tools.

Content nodes inside conditional/sequence arms (both the branchless implicit
first arm and explicit `- cond:`/`- else:` arms) now emit their own `content`
span, nested within the construct's mark, instead of being covered only by
the whole-construct `Conditional`/`Sequence` mark. Top-level prose and the
construct-extent spans themselves are unchanged.

This also changes compiled output: `LineEntry.source_location` (part of
`StoryData`'s line table, reachable through `EditorSession`/`@brink-lang/web`)
is now populated for arm content lines that previously had none.

Two further editor-observable consequences (surfaced by review):

- Prose-only arms now also project their `ConditionalBranch`/
  `SequenceBranch` CONTAINER spans: `hir_projection`'s `stmt_extent`
  derives a branch's extent from its statements' ptrs, so a body whose
  only statements were ptr-less `Content` previously projected no branch
  container at all.
- Line-context weave classification follows: arm prose lines inside a
  choice body report `w=1/ConditionalBranch` (depth inherited from the
  enclosing weave per `derive_weave`'s documented branch convention)
  instead of the scaffold fallback's `w=0`, and lines inside a
  `{ stopping: … }` block classify as `SequenceBranch` (weave depth 0 at
  top level) instead of being pattern-matched as `GatherContinuation` —
  the latter was a misclassification: those lines are sequence branches,
  not weave gathers.
