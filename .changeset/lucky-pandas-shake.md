---
"@brink-lang/editor": patch
---

Fix the editor text sliding sideways on the first click after load. The
detached-gutters layout marked its view with a class added directly to the
editor element, which CodeMirror owns and rewrites wholesale whenever it
rebuilds that element's attributes — as gaining focus does. The marker was
erased on the first click, the gutters fell back from absolute to their
inline sticky positioning and rejoined the layout flow, and because the
compensating content padding stayed applied the text jumped right by the
full gutter width. The marker is now published through CodeMirror's own
`editorAttributes` facet, so it is reapplied every time the attributes are
rebuilt.
