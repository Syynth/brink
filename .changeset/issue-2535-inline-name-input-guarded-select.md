---
"@brink-lang/editor": patch
---

The in-editor name prompt (`InlineNameInput` — the shared widget behind F2 inline rename and
extract-to-knot/function, issue #2535) no longer selects text you have already typed. It focuses
its input from a `setTimeout(…, 0)` scheduled while the widget is still detached, and the
`select()` that rode along was unguarded: typing during that window left your text selected, so
the next keystroke replaced it and the rename committed the wrong name — silently, since the
rename itself still succeeded. `select()` now runs only while the field still holds the value the
widget seeded it with, matching the guard `SymbolRenamePrompt` took in #2523. The deferral itself
is unchanged and still required: `render()` is called from CodeMirror's `WidgetType.toDOM()`,
which returns the element before the view inserts it, and focusing a detached element does
nothing.
