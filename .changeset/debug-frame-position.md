---
"@brink-lang/web": patch
---

Debugger D4 (issue #3182): the runtime can now report exactly where
execution is, not just which knot/stitch it's nearest to. `DebugFrame` and
`DebugSnapshot` (`brink-runtime`) gain an additive `position: Option<{
container_idx, offset }>` — a public mirror of the VM's internal call-frame
position, cross-checked against the already-public `Program::resolve_address`
in the new proof tests. The existing `location`/`current_location` strings
are unchanged.

`DebugState`'s JSON (`debug_snapshot()`/`flow_debug_snapshot()` on
`EditorSession`/`StoryRunner`) now additionally carries this same
`position` field on the snapshot and on each call-stack frame — the exact
JSON the studio's State View already parses on every refresh. This is a
wire-observable addition (new optional key), so it needs this changeset,
but nothing renders it yet: resolving `(container_idx, offset)` to a source
location and wiring it into the State View UI is separate follow-up work
(D6 `#3184` / D9 `#3187`, `docs/debugger-spec.md` §6), not part of this PR.
