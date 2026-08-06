---
"@brink-lang/web": patch
---

`packages/wasm-types`'s `SaveState` TS interface (re-exported through
`@brink-lang/web`) was missing `global_ids` (pre-existing drift) and
`suspended` (widened further by #2307/#2108) entirely. Both are now
mirrored, plus the `SuspendedFlow`/`WakePolicy`/`WakeSource` shapes
`suspended` needs (issue #2313).
