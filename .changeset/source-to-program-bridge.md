---
"@brink-lang/web": patch
---

Source→program resolvers reach the wasm bridge (W2/#3295): new
`resolveSourceLine(file, line0)` (line-based breakpoint binding via the
DebugInfo line index — no source text needed), `resolveSourceRange`
(now wrapped in TS), `hasDebugInfo()` (the honest discriminator between
"no debug info" and "nothing on that line"), `sourceMatches(file, text)`
(per-file staleness, tri-state), and `resolvePathAddress(path)`
(name-based addressing over the container table), on both
`StorySessionHandle` and `StoryRunnerHandle`, returning the new
`ProgramAddress` type.
