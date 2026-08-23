---
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

File-anchored project open model (epic #3021, ruled 2026-08-23): new
`entryIsExplicit` option on `ProjectSessionOptions` and
`MountStudioOptions`. When set, a discovered `brink.toml`'s
`[project] entry` never supersedes the host-given `entryFile` — the
#2331 precedence ("`[project] entry` beats `mountStudio`'s `entryFile`")
stands for host-supplied defaults, but a human's explicit file open is
not a default. Config discovery itself still runs (lints, conventions,
warnings all apply). Default `false`, the pre-existing behavior.
