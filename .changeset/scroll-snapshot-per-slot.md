---
"@brink-lang/editor": patch
---

Keep a tab's scroll position across a switch away and back. The slot machinery snapshotted the scroller's pixel offset and re-applied the number on remount, which is not a stable address into the document — CodeMirror estimates the height of unmeasured lines, so the same offset lands elsewhere once a fresh view measures (measured on an 8k-line file: leave at 4,124 px, return at 5,177 px). It now records `view.scrollSnapshot()`, a document position, and re-applies that; the pixel offset stays only as the fallback for a slot restored across a reload, where nothing but JSON crosses the boundary (#3559).
