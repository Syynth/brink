---
"@brink-lang/web": minor
---

Add the speculative-evaluation web binding (F4.3, part of #439): a sandboxed,
side-effect-proof fork of a running story that never mutates it, driven by
its own composable verbs.

`StoryRunnerHandle.speculate(options?)` forks a `SpeculationHandle` exposing
`goToPath`/`advance`/`advanceAsync`/`choose`/`evalFunction`/
`evalFunctionAsync`/`resumeFunctionEval`/`resumeFunctionEvalAsync`/
`resolveExternal`/`takePendingPromise`/`pendingExternalName`/`transcript`/
`externalsReport` — the composable primary surface. Externals are gated by a
caller-supplied `name -> "query" | "effect"` policy map plus a `"watch" |
"eval"` context (mirrors `brink_runtime::KindTieredHandler`): query externals
always run live; effect externals only run live under `context: "eval"` with
`liveEffects: true` armed, and otherwise fall back to the ink fallback body.
An async (`Promise`-returning) bound external is awaited transparently by the
`*Async` verbs, exactly like `StoryRunnerHandle.continueStoryAsync`.

`StoryRunnerHandle.evaluate(source, opts)` is a thin convenience over those
verbs for the common cases: a knot/stitch path (`"cellar.intro"`) is driven to
its next natural stop (a `done`/`end` line, or a `choices` line reported via
`reachedChoices` rather than picked); a function call with literal arguments
(`"check(1, 2)"`) is evaluated via `evalFunction`. Anything else (an arbitrary
expression, a non-literal argument) reports a diagnostic rather than running —
that's the Tier-1/F5 boundary (`docs/speculative-eval-spec.md`). `opts.signal`
(an `AbortSignal`) cancels an in-flight evaluation, dropping the speculation
and rejecting with an `AbortError`.

Function-evaluation results marshal through a new richer `TypedValue`
(`int`/`float`/`bool`/`string`/`null`/`list`/`divert`) instead of the
scalar-only `ExternalValue` the external-binding boundary uses — a `list`
carries its resolved member names/ordinals and a `divert` its resolved
knot/stitch destination, rather than collapsing to `null`.

Also renamed `docs/scratch-eval-spec.md` to `docs/speculative-eval-spec.md`
and threaded the speculative/`Speculation`/`speculate` naming through it and
its cross-reference in `docs/scoped-flow-state-spec.md` — it is now framed as
that plan's Tier-1 (arbitrary-expression) follow-on to the Tier-0 fork-based
`Speculation` this release ships.

The oracle corpus is unaffected — this is purely additive to the runtime and
web binding.
