---
"@brink-lang/studio": patch
---

The playground gains `?fixture=fixable`: a deterministic five-file project
whose diagnostic set is closed and deliberate — one cross-module import
(Suggested), five Safe-fixable warnings, one warning with no fixer at all,
and one Safe-fixable code turned `allow` in the project's own `[lints]`
table. It is the fixture the auto-fix end-to-end suite drives, and it makes
every auto-fix surface — the Problems row Fix button, "Fix all safe (N)",
both context menus, the palette commands, fix-on-save, and the Settings
Diagnostics Fix column — something an author can look at rather than only
something a test asserts.
