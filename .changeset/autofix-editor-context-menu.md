---
"@brink-lang/editor": patch
---

The plain-text editor context menu now offers the auto-fixes for the
diagnostic under the pointer (`docs/autofix-spec.md` §7), sharing the
code-actions menu's own `getFixes`/`applyFix` seams verbatim so a fix
offered by `Mod-.` and one offered by right-click are the same fix.

`TextMenuRequest` gains `fixActions?: FixMenuAction[]` and `FixMenuAction`
is newly exported (`label`, `code`, `tier`, `run`). `brinkStudio` now feeds
its own `getFixes`/`applyFix` options into the context-menu extension, so an
embedder already wiring those for the code-actions menu gets the entries
with no further change.
