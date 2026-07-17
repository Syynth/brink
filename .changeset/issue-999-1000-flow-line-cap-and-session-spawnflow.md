---
"@brink-lang/web": patch
---

FS-3w review-fix cluster (#999, #1000).

- **`FlowHandle.continueMaximally` is now capped**, matching the Rust
  runtime's `continue_maximally` (#999). It forwards to a new raw
  `continue_flow_maximally` wasm binding (`StoryRunner`/`WebSession`,
  backed by `Story::continue_flow_maximally_shared_with`) instead of
  looping the single-line `continue_flow` client-side without a bound. An
  infinite-emitting flow now throws at the runtime's
  `FlowInstance::LINE_LIMIT` (10,000 lines/turn) — the same
  `RuntimeError::LineLimitExceeded` shape `continueStory`'s cap already
  surfaces — instead of growing an unbounded array and hanging or
  exhausting memory on the host.
- **`StorySessionHandle.spawnFlow` now returns a `FlowHandle`**, aligned
  with `StoryRunnerHandle.spawnFlow` (#1000). `StorySessionHandle` also
  gains `flow(name)` and `continueFlowMaximally(name)` to match. Session
  consumers can now drive a spawned flow via the flow-addressed API the
  same way runner consumers already could.
