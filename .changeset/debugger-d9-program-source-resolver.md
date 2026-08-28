---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

Debugger D9 (issue #3187): the wasm bridge for D4's runtime position (#3182)
and D6's `DebugInfo` section (#3184) — the program→source resolver the
studio Location protocol's `program` space names as landing "with its
consumer" (`docs/studio-shell-spec.md` §6.1).

`@brink-lang/web`:

- `StoryRunnerHandle.resolveDebugPosition(containerIdx, offset)` and
  `StorySessionHandle.resolveDebugPosition(containerIdx, offset)` resolve a
  runtime `(containerIdx, offset)` position — exactly what `debugSnapshot()`'s
  `position`/call-stack frame `position` fields report — to the source range
  it was compiled from, via the loaded program's `DebugInfo` section. Returns
  `null`, not a throw, when the program carries no `DebugInfo` section (a
  compile without `--debug-info`) or the position doesn't resolve; callers
  must gate on program-identity checksum before trusting a non-null result
  (`docs/live-inspector-spec.md` §5's `sessionDegraded`).
- `ProgramModel`'s `KnotNode` gains `container_idx` (the container's index in
  the compiled program, matching a runtime `DebugPosition`) and its `disasm`
  changes shape from `string[]` to `{ offset, text }[]` — each decoded
  instruction now keeps the byte offset it decoded from, so a "current
  instruction" highlight in the Program Explorer has something to key on.
  This is a breaking shape change to `disasm`, gated behind the same
  `--debug-info`-independent Program Explorer feature that already ships —
  every consumer in this repo is updated in this same PR.

`@brink-lang/studio` (bundles `studio-shell`/`studio-ui`):

- `@brink/studio-shell` implements the `program` Location resolver
  (`makeProgramResolver`) and the `session → program` half of the chain
  (`resolveSessionPositionRef`), plus the `programIdx:offset` address
  encoding (`encodeProgramAddress`/`parseProgramAddress`).
- The Program Explorer (`ProgramView`) highlights the currently executing
  knot and instruction, gated on `sessionDegraded` — suppressed, not stale,
  the moment the running program's checksum diverges from the studio's
  latest compile.
