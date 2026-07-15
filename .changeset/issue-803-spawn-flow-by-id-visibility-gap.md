---
"@brink-lang/web": patch
---

Close the `spawn_flow` by-id visibility gap left by M-2b (#772/#781/#783/#796):
`Story::spawn_flow`'s `DefinitionId` entry point and `Story::spawn_flow_shared`'s
resolved `container_idx` entry point now refuse a `#@private` target with the
same `PrivateAccess` error the named-lookup paths already enforce.

- **`StorySessionHandle.spawnFlow`/`StoryRunnerHandle.spawnFlow`** (the wasm
  bindings over `brink_runtime::Story::spawn_flow_shared`, which resolve the
  target path to a `container_idx` themselves via `find_address` before
  calling in) now correctly refuse a `#@private` knot with `PrivateAccess`
  instead of silently starting a flow at it — previously a host holding (or
  resolving) a private target's address could bypass the name-based refusal
  entirely.
- Same documented dev-tooling override: `Story::set_visibility_enforcement`
  still governs both entry points, matching every other refusal surface.
