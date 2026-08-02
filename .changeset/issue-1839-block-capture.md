---
"@brink-lang/web": patch
---

#1839: `@[element(claims = "…", block)]` / `@[element(args = "…", block)]`
now capture the **following run** — terminated by a blank line or any
non-`CONTENT_LINE` (element-level) line — into a `content`-typed trailing
parameter, via a new internal `hir::Expr::Fragment` / `lir::Expr::Fragment`
lowering form (`docs/decision-log.md` 2026-08-01 "Content-as-value").
Interior lines are lowered through the ordinary body-item dispatch loop, so
a handler that would claim one of them still claims it — no special case.
Only `@[element(…, block)]`-declared handlers are affected; a declaration
with no `block` clause is byte-identical to before.

**`brink-runtime` fix, reachable through `@brink-lang/web`'s compile+play
path**: `OutputBuffer::has_content`/`ends_in_newline` (and the test-only
`ends_in_whitespace`) checked the *outer* transcript (or nothing at all)
while inside a `BeginFragment`/`EndFragment` capture, because no earlier
fragment use had ever captured more than one recognized line — every
earlier caller (`emit_slot_expr`'s call-composition pattern) composed
exactly one call's side-effect output. A multi-statement block capture is
the first thing that exercises a fragment holding several
`EndOfLine`-terminated lines, and the bug glued them together with no
separator at all. Fixed to check the active fragment's own capture buffer,
matching the existing `capture`-scoped branch. Any `@brink-lang/web`
consumer that constructs a multi-line `Value::FragmentRef` (only reachable
through this new block-capture mechanism today) is affected; ordinary
single-line fragment composition (an interior call's output, template slot
composition) is unaffected.
