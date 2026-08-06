# Yield-time terminal classifier — design writeup

Status: **BOTH RULINGS MADE 2026-08-01 — this document is now background,
not a gate.** Issue #1520 is **closed as folded into #1684**: R1 ruled that
the classifier's output *is* `Step`'s own variants, so there is no interim
shape and no standalone refactor — it lands as part of the Step migration.
R2 (split out as #1574) ruled **no**: `RanOutOfContent` keeps the deferred
fault, so the `oracle.rs:227` and #1522 extra-step allowances are now
**permanent**, and §5's "the allowance is retirable only under R2" is
answered — it is not being retired. The ran-out-of-content *message* does
split into four C#-matched variants (#1993), which is an independent axis
from fault timing. Companion of #1449 (whose harness half landed as PR
#1513) and #1522.

⚠ The proposal below is preserved as analysis — its six-sites inventory and
its reasoning remain accurate and useful to #1684's implementer — but its
line-number citations have drifted from `flow_instance.rs` and should be
re-derived rather than trusted.

This document is the "design first" artifact #1520 asks for. It
inventories what terminal classification looks like today, states what a
single yield-time classifier would have to decide, isolates the two
questions that are maintainer rulings rather than implementation choices,
reports the #1522 payoff check, and proposes a sequencing.

It deliberately stops short of prescribing an implementation. The
`needs-design` label plus the issue's own "Design first — this touches the
runtime's public step contract, and the Step migration (roadmap Track 1
step 3) is the natural window" is read here as: do not pick the shape
unilaterally.

---

## 1. What "terminal classification" is

At the end of a turn the runtime must answer two independent questions:

- **What content did this turn produce?** (the flushed text + tags)
- **Why did we stop?** (choices pending / `-> DONE` / `-> END` /
  *fell off the end of content*)

Today the second answer is produced in **six** places, in three
different shapes: five of those places are in the runtime (the table
below), and the sixth is not in the runtime at all — it is a consumer
re-deriving it by making an extra call.

## 2. Inventory

As of `origin/main`, all line numbers are in
`crates/brink-runtime/src/story/flow_instance.rs` unless noted.

| # | Site | What it decides | Shape |
|---|------|-----------------|-------|
| 1 | `advance_with_limit` step 2, line 405 | resume-with-buffered-output yield → `make_yield_line(self.status, …)` | status → `Line` |
| 2 | `advance_with_limit` step 4, line 430 | *deferred* `RanOutOfContent`: on the **next** call, if `status == Done && !flow.did_safe_exit` | `Err` |
| 3 | `vm::Stepped::Done` arm, line 519 | sets `status` from `pending_choices`, then `make_yield_line(*status, …)` | status → `Line` |
| 4 | `vm::Stepped::Ended` arm, line 542 | constructs `Line::End { text, tags }` **directly**, bypassing `make_yield_line` | ad-hoc `Line` |
| 5 | `make_yield_line`, line 1298 | the status → `Line` mapping itself, plus choice-list materialization | `Line` |
| 6 | `brink-test-harness` `termination.rs::classify_done` | re-derives site 2's decision by calling `did_safe_exit()` and, when false, **making an extra `continue` to elicit the deferred error** | `Outcome` |

Two structural observations fall straight out of the table:

- **`End` classifies eagerly; `Done` classifies in two halves.** Site 4
  never consults `make_yield_line`, and the fault half of `Done` lives a
  whole call later at site 2. `crates/brink-runtime/tests/terminal_classification.rs`
  (added alongside this document) pins exactly this asymmetry.
- **The information the deferred fault needs already exists at the
  yield.** `flow.did_safe_exit` is final by the time site 3 hands out
  `Line::Done`; site 2 is not gathering new information, it is *waiting*.

### 2b. Why the consumer probe exists at all

**Update (#1573):** `Story::did_safe_exit` (and the new
`FlowInstance::did_safe_exit`, for orchestration layers that drive a flow
directly) are no longer `#[cfg(feature = "testing")]` — they are ordinary
`pub fn`s on the production surface. `bevy-brink`, `brink-web`,
`brink-cli`'s TUI, and `brink-ide` can now read the flag directly after a
`Line::Done` instead of calling `continue` again and catching
`RuntimeError::RanOutOfContent`. This closes #1573's specific complaint
(no production-reachable predicate at all) but is deliberately **not**
the R1 classification-surface ruling below — it exposes today's
already-computed `did_safe_exit` bit as-is, without moving where or when
the classification happens (sites 1–5 in the table above, and the
deferred-fault timing in `advance_with_limit`, are unchanged). R1/R2 and
the `Step` migration itself remain open.

Previously: `Story::did_safe_exit` was `#[cfg(feature = "testing")]`
(`story/mod.rs`), enabled only by `brink-test-harness`
(`Cargo.toml`: `brink-runtime = { workspace = true, features =
["testing"] }`), so no production consumer could reach it — the only way
to learn whether a `Line::Done` was a clean `-> DONE` or a story that ran
out of content was to call `continue` again and catch the error. That
reconstruction is what #1520 (this document) still wants deleted from the
*harness's* `classify_done` (site 6), which needs more than the flag —
see its module doc in `termination.rs` for why the extra call survives
#1573 (it also materializes the oracle-comparable error string).

## 3. What a single classifier would have to produce

A yield-time classifier is a function evaluated once, where sites 3/4 are
today, returning something like:

```text
Terminal =
  | Choices { … }        // pending_choices non-empty
  | Done                 // -> DONE executed (did_safe_exit)
  | Ended                // -> END executed
  | OutOfContent         // yield with no choices and no safe exit
  | Suspended            // FS-3r; no StoryStatus counterpart exists yet
```

Sites 1–5 then become one construction and one lookup, and site 6 becomes
a read.

Note `Suspended`: `Line::Suspended` already exists on the public surface
(FS-3w) but has **no `StoryStatus` variant**, so `make_yield_line`
structurally cannot produce it. Any classifier written now has to leave a
slot for FS-3r, which is a second in-flight migration crossing the same
seam.

## 4. The two rulings

### R1 — where does the classification surface?

The classifier is only worth building if consumers can *read* it;
otherwise sites 1–5 collapse into a private helper and site 6 — the
issue's actual complaint, "reconstructed by each consumer" — survives
untouched. So R1 is unavoidable, and it has no implementation-local
answer:

- **(a) On the output type.** e.g. `Line::Done { text, tags, safe_exit }`,
  or a new `Line::OutOfContent`. Directly addresses the payoff. But
  `docs/prose-dialect-spec.md` §8d.7 **RULED** that the output enum
  becomes `Step`, and §7 rules that its terminals carry **no text**. Any
  `Line`-shaped change made now is churn the ruled migration then
  rewrites, and it forces every marshal leg (`brink-web`'s
  `value_marshal.rs`, `bevy-brink`'s `flow.rs`/`bindings/drive.rs`,
  `brink-ide`'s `extract.rs`, the TUI, the benches) through two breaking
  edits instead of one.
- **(b) A side-channel accessor.** e.g. promote a
  `FlowInstance::terminal() -> Option<Terminal>` out of the `testing`
  feature. Non-breaking and buildable today — but it deliberately adds a
  *second* way to ask "why did we stop" at the very moment §7's stated
  goal is to **separate the fused axes** into one. Whether that is a
  useful stepping stone or a wrong turn is a judgement about the target
  API, not about this refactor.
- **(c) Defer to the `Step` migration.** Build the classifier as the
  first move of that migration, where `Step`'s variants *are* the
  classifier's output and no interim shape is needed.

(c) is what the issue's own body gestures at ("the Step migration … is
the natural window"). It is recorded here as a recommendation, not a
decision.

### R2 — when does the fault fire?

Retiring #1522's diff allowance requires the classifier to raise
`RanOutOfContent` on the **same** `continue` that discovers the end, and
to **suppress the trailing text** — not merely to compute the answer
earlier. That is a behavior change to the public step contract, and it
moves the oracle. It is out of bounds for a behavior-preserving refactor
and needs sign-off on its own.

The C# reference confirms the target semantics.
`ink-engine-runtime/Story.cs` raises the error inside the `!canContinue`
branch of the *same* `ContinueInternal` call (line 502 opens
`if( !canContinue )`, line 506 gates on
`state.generatedChoices.Count == 0 && !state.didSafeExit && …`, line 512
adds `ran out of content …`), and the accumulated text is never returned
because the error path throws out of `Continue()`. Brink instead hands
the text out as `Line::Done` and faults one call later — the test
`ran_out_of_content_faults_on_the_call_after_the_done_line` pins that.

Two further deltas surface while checking this, both R2's business:

- C# selects among **four** messages by call-stack state (tunnel needs
  `->->`; function needs `~ return`; plain `ran out of content`; an
  "unknown reason" fallback). Brink's `RuntimeError::RanOutOfContent` now
  carries the same four-way `RanOutOfContentCause` split (issue #1993) —
  captured at `vm::handle_frame_exhaustion`, the same instant C# reads
  `callStack.CanPop`, and stashed on `Flow` for the deferred fault to read.
  Any move to same-call faulting should decide whether this classification
  moves with it. It does **not** change the oracle-matching math below:
  `oracle::oracle_outcome_eq` only compares the *category* of an error
  outcome (`(OracleOutcome::Error { .. }, Outcome::Error(_)) => true`,
  never the rendered message), so which of the four causes attaches was
  already unobservable to the harness before #1993 landed, and stays so
  after. #1993 also found that only `RanOutOfContentCause::Plain` is
  reachable through any call-stack shape this runtime can currently
  produce — the other three arms need a separate, deliberate fix to how
  `handle_frame_exhaustion` auto-pops an exhausted Tunnel frame (tracked in
  #2005), which is real VM semantics work with oracle-wide ripple risk, not
  a side effect of this design.
- C#'s guard also excludes `_temporaryEvaluationContainer != null`.
  Brink's equivalent isolation is the separate `begin_function_eval`
  loop, which raises `FunctionYielded` on `Stepped::Done | Stepped::Ended`
  (`flow_instance.rs` line 1077) — a fifth spelling of "we stopped", and
  one a classifier must not accidentally absorb.

## 5. The #1522 payoff check (the issue asked for this explicitly)

**Finding: the allowance becomes retirable only under R2, not as a side
effect of moving classification to the yield.**

The allowance lives in `crates/internal/brink-test-harness/src/oracle.rs`
(the block commented "When both oracle and brink end in Error, a single
trailing extra step in brink is acceptable"): when both outcomes are
`Error` and `brink.steps.len() == oracle.steps.len() + 1` with all shared
steps matching, the episode is scored as a match.

That extra step **is** the `Line::Done { text }` from site 3 that C#
never delivers. So:

- Computing the classification at the yield but still returning
  `Ok(Line::Done { … })` leaves the extra step exactly where it is — the
  allowance is still load-bearing.
- Returning `Err(RanOutOfContent)` at the yield removes the extra step
  and makes the allowance retirable for this class — and is a
  ratchet-moving change, in **both** directions potentially: any episode
  currently scored as a match *through* the allowance would newly match
  exactly, while any episode where the oracle does expect that trailing
  text would newly break. Which episodes are in each bucket is an
  empirical question that belongs to the R2 implementation PR, gated on
  the ruling.

## 6. Proposed sequencing (recommendation only)

1. **Now (this PR):** pin the seam with characterization tests so the
   contract cannot move silently. Done —
   `crates/brink-runtime/tests/terminal_classification.rs`.
2. **R1 ruling.** If (c), #1520 folds into the `Step` migration
   (roadmap Track 1 step 3 / `docs/prose-dialect-spec.md` §7) and stops
   being a standalone refactor.
3. **Classifier lands with `Step`**, absorbing sites 1–5 into `Step`'s
   own construction, with an FS-3r slot for `Suspended`. Behavior-
   preserving by construction: terminals still deliver text until R2.
4. **R2 ruling**, then a separate same-call-fault PR that re-runs the
   corpus, reports the episode delta in both directions, and retires the
   `oracle.rs` allowance if the numbers support it — folding in #1522's
   two insurance fixtures.

Splitting 3 and 4 keeps the ratchet-moving change isolated to one
reviewable PR, which is the whole reason #1449 was specified as "a pure
refactor — every insta snapshot byte-identical".
