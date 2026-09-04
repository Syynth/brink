---
"@brink-lang/editor": patch
---

The play gutter reads its host hooks once per render instead of once per
visible line. `lineMarker` runs for every line in the viewport, and it was
calling `getExecutionHighlights` and `getBreakpoints` from inside that
callback — so a host whose hook is expensive paid for it ~40 times per
keystroke (measured: 36 reads for a 60-line document, one per line CodeMirror
had in view).

Both reads are now cached per `EditorState`, which is exactly one read per
render pass: host truth reaches the gutter only through
`refreshExecutionHighlight` / `refreshBreakpoints`, and each dispatches a
transaction, so a refreshed answer arrives on the very next render; mounting
a view re-dispatches both refreshes, so a tab backgrounded while the answer
changed (a session pausing, a breakpoint toggled elsewhere) still repaints on
return. No API change.
