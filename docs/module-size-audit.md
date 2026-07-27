# Module-size audit

Standing assessment of oversized source files: which are genuinely two things
wedged into one file, and which are one cohesive thing that happens to be long.

> **A split that only moves code to make a number smaller is worse than leaving
> the file alone.** Size is a smell, not a defect. This document exists so that
> "this file is big" turns into a specific claim about *responsibilities* before
> anyone touches it.

Origin: issue #651 (the `brink-web/src/lib.rs` split) → issue #652 (audit the
next tier). First pass 2026-07-13; this document is the second pass, 2026-07-27.

## Why this document exists

The first pass of #652 was delivered as an issue comment. It worked — ten
follow-ups (#681–#690) were filed and all ten landed. But two weeks later the
comment was already misleading: files it had cleared had regrown, files it had
split had regrown *past their pre-split size*, and the largest files in the
repo were ones it had never seen.

An audit pinned to a commit decays. A checked-in document with a reproducible
method does not, so the assessment lives here from now on.

## Method

Raw `wc -l` over-reports badly, because this repo keeps unit tests inline in
`#[cfg(test)] mod tests`. Of the 43 files at ≥1,500 total lines, only 12 have
≥1,500 lines of *production* code; the rest are normal-sized modules wearing a
large test suite. Ranking by total lines puts `strict.rs` (63% tests) above
`vm.rs` (2% tests), which is exactly backwards as a refactoring priority.

So: **measure production lines, not total lines.** Count a file's
`#[cfg(test)]` blocks by brace depth and subtract them, and treat any file
under a directory literally named `tests` — at **any depth**, not only a
crate's top-level `tests/` directory — as a test file. This also excludes
in-`src/` test modules such as
`brink-syntax-native/src/parser/tests/declaration.rs` (1,793 lines). Under
this rule, of the 43 files at ≥1,500 total lines, 12 have ≥1,500 lines of
production code.

Then, for each file that survives that filter, ask three questions in order:

1. **Are there two or more responsibilities?** Not "are there two or more
   sections" — sections are how anyone organizes a long cohesive thing. The
   test is whether the parts have *different reasons to change*.
2. **Is the seam mechanical?** Can the split be a verbatim move of contiguous
   line ranges plus `mod`/`pub use` wiring, with no logic relocated out of a
   `match` arm and no signatures touched? If it needs judgement calls, it is a
   design change wearing a refactor's clothes — file it for design, don't do it
   mechanically.
3. **Does the split buy parallelism?** Two responsibilities edited by the same
   person in the same change are worth less to separate than two edited by
   different workstreams that collide today. This is the tiebreaker, and it is
   the reason the exemplar below was chosen over larger files.

A file that fails (1) is **inherent** — leave it. A file that passes (1) but
fails (2) is **needs-design**. A file whose overage is mostly inline tests is a
**test-extraction** candidate, which is a different and much cheaper operation
than a production split.

## Current assessment (2026-07-27)

Production lines, excluding inline `#[cfg(test)]` blocks.

| File | Total | Prod | Verdict |
|---|---:|---:|---|
| `brink-lsp/src/backend.rs` | 3,645 | 3,162 | **split** — regrew past its own #688 split |
| `brink-runtime/src/vm.rs` | 2,675 | 2,616 | **needs-design** — one dispatch loop |
| `brink-db/src/queries/mod.rs` | 2,601 | 2,601 | **split** — query catalogue, clusters visible |
| `brink-syntax/src/ast/nodes.rs` | 2,318 | 2,318 | **inherent** — generated-style accessors |
| `brink-analyzer/src/infer/body.rs` | 2,272 | 2,272 | **needs-design** — one 1,854-line `impl` |
| `brink-format/src/opcode.rs` | 2,914 | 2,163 | **inherent** — one instruction set |
| `brink-ir/src/lir/lower/mod.rs` | 2,063 | 2,063 | **split** — two self-contained post-passes |
| `brink-ir/src/lir/lower/expr.rs` | 2,156 | 2,061 | **inherent** — one expression lowering |
| `brink-ir/src/hir/diagnostics.rs` | 1,749 | 1,749 | **inherent** — new; see exemplar below |
| `brink-ir/src/hir/types.rs` | 1,433 | 1,433 | **split (done, this PR)** — was 3,170 prod, the largest production file in the repo before this PR; now below the ≥1,500 threshold, see exemplar below |
| `brink-analyzer/src/strict.rs` | 4,111 | 1,539 | **test-extraction** — 63% inline tests |
| `brink-ir/src/hir/emit_native.rs` | 1,517 | 1,513 | **inherent** — one emitter |
| `brink-test-harness/src/bin/compile_bench.rs` | 1,506 | 1,506 | **inherent** — a bench binary |

Everything else at ≥1,500 total lines has under 1,500 production lines and is
**not** a production-split candidate. The largest of those are worth naming,
because their raw line counts keep drawing attention:

| File | Total | Prod | Note |
|---|---:|---:|---|
| `brink-web/src/editor/mod.rs` | 4,212 | 1,042 | 75% inline tests |
| `brink-runtime/src/collection_ops.rs` | 2,476 | 1,226 | 50% inline tests |
| `brink-analyzer/src/resolve.rs` | 2,290 | 1,217 | 47% inline tests |
| `brink-format/src/value.rs` | 2,174 | 1,318 | 39% inline tests |
| `brink-analyzer/src/lib.rs` | 1,999 | 1,345 | 33% inline tests |
| `brink-runtime/src/story/mod.rs` | 2,161 | 1,445 | 34% inline tests; direct descendant of #652's #1 charter entry (`story.rs`, 3,503) — see charter reconciliation below |
| `brink-runtime/src/value_ops.rs` | 3,061 | 1,375 | 55% inline tests |
| `brink-analyzer/src/infer/mod.rs` | 2,905 | 1,054 | 64% inline tests |
| `brink-runtime/src/world.rs` | 3,078 | 1,448 | 53% inline tests; cleared in pass 1, still clear |
| `brink-db/src/db.rs` | 2,278 | 824 | 64% inline tests |
| `bevy-brink/src/source_loader.rs` | 2,012 | 533 | 74% inline tests |

The five files above the fold (`collection_ops.rs` through `story/mod.rs`) are
all larger — by both total and production lines — than `db.rs` and
`source_loader.rs`, which were already named. They are listed here for the
same reason: raw size keeps drawing attention, and each is confirmed
production-under-threshold, not a split candidate.

`bevy-brink/src/sleep/tests.rs` (2,461) is a test module in a non-`tests/`
path, so the production-line heuristic misclassifies it; it is a test file.

### Assessed, leave alone — with reasons

- **`ast/nodes.rs` (2,318)** — ~100 one-liner `impl XxxNode` accessor blocks,
  one per AST grammar node. Cohesive by construction. Pass 1 reached the same
  conclusion and it still holds; the growth since is new grammar nodes, which
  is the file doing its job.
- **`opcode.rs` (2,163 prod)** — one `Opcode` enum, its byte constants, and its
  encode/decode/display tables, plus the operand sub-enums (`TowerOp`,
  `CollectOp`, `SequenceKind`, `ChoiceFlags`) and `DecodeError`. It grew from
  1,598 because the instruction set grew. That is inherent: an instruction set
  is one thing, and scattering it across files makes it harder to see, not
  easier. Leave it.
- **`vm.rs` (2,616 prod)** — almost entirely the central opcode-dispatch
  `step()` loop. It already extracts named helpers for the complex opcodes.
  Splitting further means carving inline `match`-arm bodies into new files,
  which relocates logic — it fails question (2). Pass 1 flagged this and the
  flag stands: **design discussion before anyone touches it**, not a mechanical
  split.
- **`infer/body.rs` (2,272 prod)** — a single `impl InferPass` of 1,854 lines.
  Rust permits splitting an inherent `impl` across modules in one crate, so
  this *could* be mechanical, but only after the struct's fields get visibility
  bumps and the method groups are shown to be genuinely disjoint. Same class as
  `vm.rs`: needs design, not a line-range move.
- **`lir/lower/expr.rs` (2,061 prod)** — one lowering pass over one IR node
  family. Long because expressions have many forms.
- **`strict.rs` (1,539 prod)** — production is barely over threshold and has
  two mild clusters (void-checking, temp collection). The 2,572 lines of inline
  tests are the actual bulk. Extract tests first; reassess after.

### Charter reconciliation (#652)

#652 named eight files by path when it was filed (2026-07-13), plus an
unenumerated "1,500-line tier". Two weeks of unrelated work have since
renamed, split, or shrunk several of them. One line per named file, traced to
its current state:

| #652 path | Current path | Total | Prod | Verdict |
|---|---|---:|---:|---|
| `brink-runtime/src/story.rs` | `brink-runtime/src/story/mod.rs` | 2,161 | 1,445 | test-extraction — see second table above |
| `brink-runtime/src/world.rs` | unchanged | 3,078 | 1,448 | test-extraction — see second table above |
| `brink-cli/src/ide.rs` | split into `ide/{mod,handlers,commands,project}.rs`; largest is `ide/project.rs` | 1,686 | 1,070 | already resolved by an unrelated prior split — test-extraction, under production threshold |
| `brink-lsp/src/backend.rs` | unchanged | 3,645 | 3,162 | **split** — see main table above |
| `brink-syntax/src/ast/nodes.rs` | unchanged | 2,318 | 2,318 | **inherent** — see "assessed, leave alone" above |
| `brink-fmt/src/lib.rs` | unchanged | 1,294 | 65 | already resolved — shrunk well below threshold by unrelated prior work |
| `bevy-brink/src/bindings.rs` | unchanged | 71 | 63 | already resolved — shrunk well below threshold by unrelated prior work |
| `brink-ide/src/structural_move.rs` | unchanged | 1,621 | 1,004 | test-extraction — under production threshold |

Of the eight, two (`backend.rs`, `ast/nodes.rs`) are covered by the main
verdict table, one (`world.rs`) by the second table, and one (`story.rs`, now
`story/mod.rs`) has been added to the second table above by this pass. The
remaining four either already resolved on their own (`ide.rs`, `fmt/lib.rs`,
`bindings.rs`) or are newly recorded here as test-extraction candidates
(`structural_move.rs`). No file in the charter is unaccounted for.

The unenumerated "1,500-line tier" is the rest of this document: every file
≥1,500 total lines in the repo today is measured somewhere above, split
between the two verdict tables.

## Exemplar: `hir/types.rs` → `hir/types.rs` + `hir/diagnostics.rs`

Landed with this document as the worked example of what a *good* split looks
like under the method above.

`brink-ir/src/hir/types.rs` was 3,170 lines with zero tests, and it held two
things behind its own `// ─── Diagnostics ───` section divider:

- **HIR node definitions** (lines 1–1,433) — the structs and enums for knots,
  stitches, blocks, statements, expressions, declarations.
- **The diagnostic catalogue** (lines 1,437–3,170) — `Diagnostic`, `Severity`,
  the 1,093-line `DiagnosticCode` enum, and its 594-line
  `as_str`/`from_str`/`severity` lookup tables.

Against the three questions:

1. **Two responsibilities — yes, and unusually cleanly.** Every language-feature
   change edits the node types; every diagnostic-adding change edits the code
   catalogue. Different reasons to change, and the author had already drawn the
   line with a section divider.
2. **Mechanical — yes, maximally.** `mod types` was already private and
   re-exported via `pub use types::*`, so the split is invisible outside `hir/`:
   no consumer import changed, across the 147 files that reference
   `DiagnosticCode`. Only two names cross the seam (`FileId`, `TextRange`);
   everything else that looked like a dependency was prose in doc comments.
   Both halves are byte-identical to the corresponding line ranges of the
   pre-split file — the only authored lines are the new module header and two
   lines in `mod.rs`.
3. **Parallelism — the highest in the tree.** The diagnostic catalogue is the
   single most-contended file in the compiler: adding any diagnostic means
   appending a variant and three match arms, and that collides with every
   concurrent language-feature change touching the node types next door.

That third point is why this file was chosen ahead of `backend.rs` (3,162 prod
lines, nearly twice as big). Bigger is not the same as more valuable to split.

## Follow-ups

Filed against the **split** verdicts above; see the issue tracker for
`brink-lsp/src/backend.rs`, `brink-db/src/queries/mod.rs`, and
`brink-ir/src/lir/lower/mod.rs`. The **needs-design** verdicts (`vm.rs`,
`infer/body.rs`) are deliberately not filed as refactor issues — they need a
design decision first.

## Keeping this current

Re-run the production-line measurement before trusting the table above; the
2026-07-13 pass went stale in two weeks. Treat any verdict older than a release
cycle as a hypothesis rather than a finding.

Two standing rules, both inherited from #652:

- Structural hygiene is a **review dimension**, not an in-PR licence. Reviewers
  flag a touched file that has outgrown one responsibility as a scope-gap
  follow-up; they do not ask for the refactor in that PR.
- A split PR is **mechanical or it is not a split PR**. Verbatim line-range
  moves, `mod`/`pub use` wiring, visibility bumps only where the compiler
  demands them. No logic changes riding along, and the oracle ratchet must not
  move in either direction.
