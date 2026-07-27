---
"@brink-lang/web": patch
---

#1335 (B0.8b): closes several `brink_ir::hir::emit_native` construct-coverage
gaps discovered by re-checking the issue's gap list against current
`main` and a full-corpus diagnostic sweep (`brink-respell`'s new
`full_corpus_sweep.rs` test):

- Two emitter completeness bugs, not native-grammar gaps — a choice body
  can absorb a same-line divert *and* further statements (a leading
  `[Divert, EndOfLine, …]` shape only a two-element pattern was matching
  before), and an `else`/fallback choice with a bare `-> target` body (no
  display text at all) had no same-line-divert spelling; both now emit
  via the general braced-block form.
- A bare `(name)` label immediately followed by a `{?}` choice point (a
  `Stmt::LabeledBlock` whose first statement is a `ChoiceSet`, not
  `Content`) now emits — the labeled-line dispatcher only recognized a
  `Content`-leading shape before.
- `Import` (`use`/`import` declarations) is spelled back instead of
  refused outright — issues #1581/#1590 already fixed `Import.module` to
  be the real `::`-joined module name upstream of this emitter, so the
  blanket refusal predates that fix.
- A newly-discovered silent-drop bug: `HirFile::allow_scopes`
  (`@[allow(…)]` suppression scopes, issue #1614/#1161) was never
  checked, so a file using it would round-trip with its suppression
  quietly gone. Now refused loudly instead.

Not wired into any compile/analysis path — same posture as #1178's and
#1335's first changeset: `emit_native` is called only by `brink-respell`'s
own tests (dev-only, `publish = false`, never shipped). No behavior change
for any existing `.ink` or `.brink` session; this only shrinks the
emitter's own refused-construct set.

The full-corpus sweep (~396 oracle cases) still cannot mechanically
respell the whole corpus end to end: 187/396 now succeed (up from 177),
with the remaining ~209 blocked overwhelmingly by missing **native
grammar** (not emitter gaps) for prose-body code-ground statements
(`~ x = expr`-style assignment/temp-decl/expression-statement/thread-start
splices, a function body's `return` with a value, `else if` chains),
alternations (grammar exists but the emitter itself never grew the arm —
a real, separately-scoped follow-up), and `INCLUDE` files. See the PR
body for the full breakdown and two additional findings (an `E033`
dead-code true-positive and a root-content addressing mismatch) that are
real but out of this slice's scope.
