---
"@brink-lang/web": minor
---

FS-3w — flow-addressed web surface (slice 1 of FS-3, issue #978).

New API surface, shipping against today's runtime so consumers migrate the
interface shape early (FS-3r later changes behavior, not interface):

- **Flow handles.** `StoryRunnerHandle.flow(name)` and the new return value
  of `spawnFlow(name, path?)` are addressable `FlowHandle` objects — each
  spawned/ambient flow has its own `Line` stream via `continue()` /
  `continueMaximally()`, plus `choose()`, `debugSnapshot()`, and
  `destroy()`. Thin views over the existing name-addressed flow methods.
- **Story-level drive is documented sugar for the primary flow.**
  `continueStory` / `continueSingle` (and the async variants) drive the
  always-present default flow. No behavior change — existing consumers are
  unchanged.
- **`Line` gains the `"suspended"` type** (a flow parked at an `await`).
  Runtime-unreachable until FS-3r — the E052 fence keeps `await` from
  lowering, so nothing constructs it today; it ships now purely so the API
  shape is stable.
- **`wakeCheck()`** (on `StoryRunnerHandle`, `StorySessionHandle`, and the
  raw `WebSession`) re-evaluates parked flows' wake conditions and returns
  the woken flow ids. Returns an empty list until parks exist (FS-3r);
  dirty-tracking is not built here.
