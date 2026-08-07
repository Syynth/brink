---
"@brink-lang/studio": patch
---

`mountStudio`'s returned `StudioHandle` now exposes `entryFile`: the
project's EFFECTIVE entry file (`ProjectSession.getEntryFile()`'s result,
with `[project] entry` precedence already applied per issue #2331), read
once `initialize()` has run. A host that needs to act on "the file the
editor actually treats as the entry" (batch tooling, an export command)
should read this instead of echoing back its own `MountStudioOptions.entryFile`
argument — that argument is only the fallback for a configless project, and
a host using it directly could silently disagree with the editor for any
project whose `brink.toml` names a different entry (2026-08 review finding
on brink-desktop's `exportXliff`, #2392).
