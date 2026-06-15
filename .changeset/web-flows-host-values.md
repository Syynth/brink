---
"@brink-lang/web": minor
---

Add program-identity, flow-control, and host-value APIs.

- `programChecksum(bytes)` — the source-identity checksum of compiled `.inkb`
  bytes (matches `ProgramModel.checksum`) without constructing a runner.
- Shared-context flows on `StoryRunnerHandle`: `spawnFlow`, `continueFlow`,
  `chooseFlow`, `destroyFlow`, `flowNames`, `flowDebugSnapshot` — concurrent
  flows of one story that share globals / visit counts / rng.
- `EditorSessionHandle.setHostValues` / `clearHostValues` — push host-provided
  values for `host`-source semantic types into the editor's value cache (the
  author-time argument picker).
