---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Suppressing a diagnostic project-wide now clears its squiggle immediately,
instead of leaving it until the file is edited or reopened.

Compile squiggles are published by the diagnostics extension's ViewPlugin,
which wakes on a document change. A compile that lands for some other reason
— a `brink.toml` edit changing `[lints]`, a suppression written into a
sibling file — has no document change in the view showing the diagnostic, so
nothing republished. The prose checker already had this seam
(`refreshProseEffect`); `refreshDiagnosticsEffect` is its compile-side twin,
dispatched wherever a compile is delivered.
