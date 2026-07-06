---
"@brink-lang/web": minor
---

Expose the Rust `StorySession` journal/replay layer (#370, PR #385) on `@brink-lang/web` as `StorySessionHandle` (#387): `advance`/`continueSingle`/`continueToPause`, `choose`/`resolveExternal`, turn-boundary `setVar`/`goToPath`/`saveState`/`loadState`, journaled `callFunction`, `snapshot`/`diff` (+ standalone `diffSnapshots`), `exportJournal`/`StorySessionHandle.restore`, `reload`/`continueReplay`, and `restart`. Fixes the wire-format lie where `awaiting_external` was smuggled into the `Line` union: `advance()` now returns a distinct `StepOutcome` (`{ type: "line", line } | { type: "awaiting_external", deferred, name? }`), keeping the two park states (promise-in-flight vs. deferred out-of-band) explicit. New TS types (`StepOutcome`, `SessionJournal`, `StateSnapshot`, `StateDiff`, `ReplayOutcome`, etc.) ship from `@brink/wasm-types` and are re-exported here.
