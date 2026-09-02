---
"@brink-lang/editor": patch
---

The code-actions menu (Ctrl-. / Cmd-.) now lists the auto-fixes for the
diagnostics under the cursor, above the structural entries. One click applies
one fix through the host's existing apply seam, and a `placeholder` fix moves
the caret into the hole it left.

`BrinkStudioOptions` gains `getFixes`, `applyFix` and `resolveFixCaret`;
`DocumentSessions` wires all three, so an embedder using it gets the fixes
with no change. A document handle gains `fixes(offset)` and `applyFix(fix)`.
