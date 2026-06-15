# @brink-lang/web

## 0.3.0

### Minor Changes

- bcd23b7: Add program-identity, flow-control, and host-value APIs.

  - `programChecksum(bytes)` — the source-identity checksum of compiled `.inkb`
    bytes (matches `ProgramModel.checksum`) without constructing a runner.
  - Shared-context flows on `StoryRunnerHandle`: `spawnFlow`, `continueFlow`,
    `chooseFlow`, `destroyFlow`, `flowNames`, `flowDebugSnapshot` — concurrent
    flows of one story that share globals / visit counts / rng.
  - `EditorSessionHandle.setHostValues` / `clearHostValues` — push host-provided
    values for `host`-source semantic types into the editor's value cache (the
    author-time argument picker).

## 0.2.0

### Minor Changes

- 20764ef: Add `StoryRunnerHandle.goToPath(path)` — ink's `ChoosePathString` equivalent. Moves the play head to a named knot or stitch (`"knot"` / `"knot.stitch"`); subsequent `continue*` calls run from there. The session keeps its state: variables and visit/turn counts survive, and the jump itself counts as a visit to the target, exactly like a `-> path` divert. Pending choices are abandoned (callstack reset); the transcript so far is kept. Throws on an unknown path (naming it), and refuses to jump while the story is parked on an unresolved async external — resolve it (or `reset()`) first.
