# Native grammar holes re-triage (issue #1951, 2026-08-01)

Issue #1951 tracked six "native grammar holes" transcribed from #1335's
2026-07-27 comment, framed as the reason `brink-respell`'s full-corpus sweep
stands at 187/396. The issue's own body flagged that the list was
transcribed, not re-derived, and asked for each hole to be verified
independently before any grammar work started. This document is that
verification.

## Method

Each hole was probed with an isolated one-file-per-project `.brink` fixture
(`brink.toml` with `dialect = "brink"` sitting beside a single `story.brink`
— native discovery is tree-is-universe: multiple `.brink` files in one
directory silently merge into one project with duplicate `flow main()`,
which produces nonsense results if you don't isolate). Every probe was run
through `brink compile <file> -o <file>.inkb -D warnings` (promoting every
Warning-severity diagnostic to a hard error, so nothing is hiding below the
default log level) using the CLI's now-fixed diagnostic renderer (#1957 —
`path:start..end [CODE] message`, not just a count). Where compilation
succeeded, the resulting `.inkb` was also run through `brink play -s 0` to
check that the compiled behavior actually matches the source's apparent
intent — **compiling clean is not sufficient evidence a construct "works":
two of the six holes compile clean while silently discarding the author's
intent**, which a `brink compile`-only check would have missed entirely
(see Hole 1 below; this is exactly the shallow check the 2026-08-01 comment
on #1951 made and got wrong).

Ground truth for corpus impact came from `brink-respell`'s existing,
`#[ignore]`d full-corpus sweep:

```sh
cargo test -p brink-respell --test full_corpus_sweep -- --ignored --nocapture
```

Run today against `origin/main` plus this triage's doc fixes (no production
code changed), it reports:

```
oracle cases:  397
respell OK:    187
respell FAIL:  210
```

(397, not 396 — one fixture has been added to the corpus since #1335's
187/396 was last quoted; the 187 "OK" count is unchanged.) The 210 failures
bucket into exactly 16 distinct `EmitError::Unsupported` reasons. Seven of
those buckets map directly onto the six named holes — Holes 1 and 2 together
cover three (`temp declaration`, `assignment`, `expression statement`); nine
do not appear in #1951's list at all (see "Beyond the six holes" below).

## Verdicts

### Hole 1 — "No native spelling for `~ x = expr`"

**REPRODUCES — and the 2026-08-01 comment's "does not reproduce" verdict on
this exact hole was itself wrong**, because it stopped at "compiles clean"
without checking whether the assignment actually took effect.

```brink
var n = 0
flow main() {
  n = 1
  Value is {n}.
  -> END
}
```

compiles with **zero diagnostics**, even under `-D warnings`. But running
the compiled story:

```
$ brink play story.inkb -s 0
n = 1
Value is 0.
```

`n = 1` is not parsed as an assignment at all — `block.rs::body_line` has no
dispatch arm for it at content-ground (prose-body) position, so it falls
through to `content::content_line` and is folded into an ordinary `TEXT`
run. It prints as literal story text and `n` never changes. This is a
**silent grammar gap**, worse than a compile error: the author gets no
diagnostic and no execution failure, just a story that quietly does the
wrong thing. At the time of writing, this was confirmed by
`hir::emit_native`'s own module doc, which independently reached the same
conclusion: "`Stmt::TempDecl`/`Assignment`/`ExprStmt`/`LogicBlock`/`Await`
at prose-body position ... this is a native-**grammar** gap, not just an
emission one: there is no bare `~ x = expr`-style prose-body statement to
round-trip yet."

**Correction (issue #1991, PR #2002, landed after this triage):** that
quote is now stale for two of the five variants it named. `~ x = expr` /
`~ expr` (the ruled content-ground logic-line escape, charter §8.2) is
exactly the bare prose-body statement the old sentence said didn't exist —
it does now, parsed by `stmt::logic_line` and lowered by
`lower_native::body::lower_logic_line` to real top-level
`Stmt::Assignment`/`Stmt::ExprStmt` HIR, with zero diagnostics. `emit_native`
itself has **not** grown a printer for either (the corpus impact below is
unchanged by #2002 — it measures the printer/`brink-respell` direction, not
the parser one this fix touched), so its own module doc now classifies both
as emitter-only, the same bucket its alternations sentence already used —
see that doc for the corrected wording. `TempDecl`/`LogicBlock`/`Await`
remain genuine native-**grammar** gaps: #1991/#2002 scoped only the
`~`-prefixed assignment/bare-call spelling, and the fixtures below use the
**bare, unprefixed** form (`n = 1`, no `~`), which is a different, still-
unfixed gap — tracked at **#1972** ("prose-body statement grammar:
assignment/call/temp-decl/logic-block/await"), OPEN as of this writing.

Corpus impact: the `full_corpus_sweep` `"assignment"` bucket alone is **27
cases**.

### Hole 2 — "Bare call statements"

**REPRODUCES**, same root cause and same silent-swallow behavior as Hole 1:

```brink
var n = 0
fn bump() {
  n = n + 1;
  return 0;
}
flow main() {
  bump()
  Value is {n}.
  -> END
}
```

compiles with zero diagnostics; `brink play` prints `bump()` as literal text
and `Value is 0.` — the call never executes. Same code-location cluster as
Hole 1 (`Stmt::ExprStmt` has no prose-body grammar either, for the *bare*
spelling probed here — see the #1991/PR #2002 correction on Hole 1: the
`~`-prefixed spelling now does).

Corpus impact: `"expression statement"` bucket, **19 cases**.

**Holes 1 and 2 are the same underlying gap** — `emit_native`'s module doc
used to group `TempDecl`/`Assignment`/`ExprStmt`/`LogicBlock`/`Await` under
one finding, because `lower_native::body` never constructed any of them
outside a `~{ }` logic block, full stop. As of #1991/PR #2002 that's no
longer true for `Assignment`/`ExprStmt` at the **lowering** level (the
`~`-prefixed spelling constructs them directly, no `~{ }` needed) — but the
bare, unprefixed spelling these two holes actually probe is untouched, and
`emit_native`'s **printer** hasn't grown support for either variant either
way, so the corpus buckets below are unchanged. The single biggest lever in
the whole sweep is still fixing the remaining gap (bare prose-body
statement recognition, #1972, plus growing `emit_native`'s printer):
`"temp declaration"` (14) + `"assignment"` (27) + `"expression statement"`
(19) = **60 cases**, more than a third of all remaining failures, from one
prose-body statement form.

### Hole 3 — "`<- flow(args)` outside `~{}`"

**REPRODUCES exactly as literally worded, but it is a *deliberate, already-
ruled* restriction (#1260/#1263, charter §11), not an unruled grammar gap —
and it is very likely NOT the thing actually blocking respell coverage.**

```brink
flow helper(x: int) {
  Helper got {x}.
  -> END
}
flow main() {
  <- helper(5)
  Hello.
  -> END
}
```

produces, under `-D warnings`:

```
[E131] `<-` outside a choice point has no effect here; it is treated as
ordinary text, not a splice. The tokens after `<-` look like a knot/flow
reference (`<- name` / `<- name(args)`) — if this was meant as a splice,
move it inside a `{? … }` choice point (charter §11); splices are only
recognized there.
```

`choice.rs::splice_outside_choice_point`'s own doc comment cites the exact
ruling: "charter §11 narrows threads to scoped splices inside choice
points" (#1263, ruled #1260 on #1256). Extending this would mean
overturning a settled design ruling, not filling an oversight — per this
wave's own instruction, that needs a fresh design decision, not an
implementation ticket.

**But** a real ink story almost never writes `<- thread(args)` outside a
choice point — ink's documented use of threads is a sibling of the choice
lines around it, inside `{? … }`. The `full_corpus_sweep`'s
`"thread-start splice"` bucket (**21 cases** — e.g.
`I077-fallback-choice-on-thread`, `I091-choice-count`) is almost certainly
all *inside-choice-point* splices, which native's grammar already spells
fine. What blocks them is a **separate, narrower emitter gap**: HIR
flattens a `Stmt::ThreadStart` into an ordinary preceding/trailing
statement with no marker of its original nesting inside the choice point,
and nothing in `emit_native.rs` re-nests it on the way back out
(`Stmt::ThreadStart(_) => return Err(unsupported("thread-start splice",
context))`, confirmed by grepping `emit_native.rs` for that refusal string
— line numbers drift with every merge, so this doc cites the string, not a
line). This is real, open, and
**pure emitter work — no grammar change, no design ruling needed** — but it
is a different gap than Hole 3 as worded in #1951, which is why it gets its
own corrected issue below rather than reopening #1260.

### Hole 4 — "No value-carrying `return` at body position"

**REPRODUCES.**

```brink
flow main() {
  Hello.
  return 5
}
```

fails even without `-D warnings`:

```
[E033] unreachable code after divert
```

`divert.rs::return_stmt` (the content-ground/prose-body form) only ever
parses a bare `return` or the tunnel-redirect `return -> target` — it never
consumes a trailing expression, so `5` is left as dangling, unreachable
content after what the grammar treats as a terminal exit. A bare `return`
(no value) compiles clean, confirming the value expression specifically is
what's missing. Matches `emit_native`'s own finding: "a prose-body `return`
with a value expression ... `RETURN_STMT` never carries a value at body
position."

Corpus impact: `"return with a value expression"` bucket, **16 cases**.

### Hole 5 — "No `else if` chain"

**DOES NOT REPRODUCE as a grammar gap — the hole as stated is false.** The
real, narrower, surviving gap is emitter-side, and two stale doc comments in
this repo are very likely the actual source of the mis-transcription (fixed
in this PR, see below).

```brink
var score = 2
flow main() {
  {if score == 1: One.
  else if score == 2: Two.
  else: Other.}
  -> END
}
```

compiles with zero diagnostics under `-D warnings`, and `brink play`
correctly prints `Two.` — the chain parses and executes correctly. Native's
parser has supported a flat `else if <cond> { … }` / `else if <cond>: …`
chain since **#1258/#1261 (2026-07-22)**, at both content-ground
(`family.rs::else_branch`) and code-ground (`control_flow.rs::else_clause`)
— both have direct proptest and unit-test coverage
(`brink-syntax-native/src/parser/tests/brace_family.rs`:
`else_if_flat_chain_is_recognized_as_a_chain`,
`else_if_flat_chain_colon_form_is_recognized_as_a_chain`,
`conditional_colon_and_else_if_flat_parse_clean`).

What survives: `lower_native::cond::lower_conditional`'s own doc comment
(**before this PR**) said "the grammar has no `else if` chain" — stale,
predating or never reconciled with #1258/#1261 — and
`hir::emit_native`'s module doc and `b07_native_body.rs`'s cross-frontend
test doc repeated the same stale claim. **This triage fixes all three
comments** (pure doc correction, zero behavior change — see the diff).

The real, still-open gap: a native `else if` chain lowers through
*nesting* (an `else` arm's body containing another `Conditional`), not a
flat multi-branch list, so `emit_native::emit_conditional` — which only
handles `CondKind::InitialCondition` with **exactly one** condition-bearing
branch plus an optional plain-else — has no path to reshape ink's
`CondKind::IfElse` (ink's own independently-chained 3+-branch, no-shared-
subject form) into that nesting on the way out. Confirmed by the
`CondKind::IfElse => Err(unsupported("IfElse conditional (no native
\`else if\` chain)", context))` arm in `emit_native.rs`'s `emit_conditional`
(cited by refusal string, not line number, since that drifts with every
merge).

Corpus impact: `"IfElse conditional"` bucket, **12 cases**.

### Hole 6 — "`ContentPart::Spring`"

**REPRODUCES as a genuine grammar gap**, but it's a different shape of gap
than 1/2/4: it is unreachable from any native-authored `.brink` source at
all (there is nothing to probe with `brink compile`), and confirmed purely
by source inspection rather than a live probe.

`ContentPart::Spring` is a deferred word-break marker
(`hir::lower::choice::replace_trailing_ws_with_spring`) the **ink frontend
only** inserts when a choice's start-content ends in whitespace right
before the `[bracket]` — it lets the runtime decide at render time whether
a space is needed, since whether the bracket/inner content is empty isn't
known until the choice is actually taken. `hir::lower_native::choice` has
no equivalent production anywhere — a `.brink` author literally cannot
write anything that becomes a `Spring` — so this is entirely an
ink-sourced-HIR emission concern, not a "compile this native construct and
see it fail" one, and `brink compile` never touches this code path. Per
`emit_native`'s own finding: "no native token forces that same
runtime-deferred-whitespace behavior, and respelling it as a literal space
would silently change what renders, so it stays refused" — the
`ContentPart::Spring => return Err(unsupported("word-break spring",
context))` arm in `emit_native.rs` (cited by refusal string, not line
number, since that drifts with every merge).

Corpus impact: `"word-break spring"` bucket, **12 cases**. Whether native
should grow a new token/behavior to express this, or whether these 12 cases
should be accepted as permanently out of respell scope, is a **design
question** — flagged, not decided, per this wave's instruction not to
invent a ruling.

## Beyond the six holes

The six named holes account for 27 (assignment) + 19 (expression statement)
+ 14 (temp declaration — folded into Holes 1+2, see above) + 21
(thread-start splice) + 16 (return with a value expression) + 12 (IfElse
conditional) + 12 (word-break spring) = **121** of the 210 failures, across
seven `EmitError::Unsupported` buckets (and that's counting "thread-start
splice" and "IfElse conditional" generously, under the corrected framing
above — the original Hole 3/Hole 5 wording maps to neither bucket cleanly).
The other **89 failures**, across nine buckets, were never named in #1335's
comment or #1951's body:

| Bucket | Count | Example |
|---|---|---|
| `inline conditional in content` | 21 | `tests/tier1/choices/I087-non-text-in-choice-inner-content` |
| `divert-target-as-value expression` | 18 | `tests/tier1/diverts/I056-divert-targets-with-parameters` |
| `alternation sequence` | 17 | `tests/tier1/diverts/I063-divert-to-weave-points` |
| `inline sequence in content` | 10 | `tests/tier2/sequences/I108-blanks-in-inline-sequences` |
| `INCLUDE sites` | 9 | `tests/tier1/includes/root-weave-in-entry-and-included-file` |
| `list literal expression` | 8 | `tests/tier2/lists/I067-list-save-load` |
| `multi-hop tunnel chain` | 3 | `tests/tier1/diverts/I062-complex-tunnels` |
| `tunnel-return onwards args` | 2 | `tests/tier1/diverts/I060-tunnel-onwards-divert-after-with-arg` |
| `match arm with no pattern` | 1 | `tests/tier2/evaluation/I122-evaluation-stack-leaks` |

Notably, `alternation sequence` (17) + `inline conditional in content` (21)
+ `inline sequence in content` (10) = **48 cases** are, per
`emit_native`'s own module doc, **pure emitter gaps with no grammar or
design work needed at all** — "native's `ALTERNATION_BLOCK` parses and
`lower_native::body`/`expr` already lower it to real `Sequence`/
`InlineSequence`/`InlineConditional` HIR today ... this emitter has simply
never grown the `emit_*` arm for it." That's a bigger, lower-risk lever
than any of the six named holes, and it isn't tracked as its own issue
anywhere — recorded here and in a comment on #1951 so it isn't lost the way
untracked partial findings have been before.

This document does not re-triage these nine buckets individually (out of
scope for this pass); a future wave picking up native-grammar-hole work
should start from `full_corpus_sweep`'s live output, not from #1335's
comment or this document's summary, since both can go stale the moment a
fix lands elsewhere — the sweep can't.

## Filed issues

| Verdict | Issue |
|---|---|
| Holes 1+2 (prose-body statement grammar: assignment/call/temp-decl/logic-block/await) | #1972 |
| Hole 3 corrected (thread-start splice re-nesting, emitter-only) | #1974 |
| Hole 3 literal wording (outside-choice-point splice) | **not filed** — deliberate ruling #1260, would need a design re-ruling, not implementation |
| Hole 4 (prose-body value-return) | #1973 |
| Hole 5 corrected (`CondKind::IfElse` emitter gap) | #1975 |
| Hole 6 (`ContentPart::Spring`) | #1976 — flagged needs-design on whether native should grow new syntax for this at all |
