---
"@brink-lang/editor": minor
---

Exported `scheduleIdleWork`/`cancelIdleWork` (the off-paint-path scheduling helper #722 added
for the inline rename widget) from the package's public entry point, so other rename/analysis
surfaces can take the same discipline instead of re-implementing it (issue #696).
