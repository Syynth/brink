---
"@brink-lang/studio": patch
---

The editor/story-graph symbol context menu (and the rename modal) now actually appear: both hosts rendered OUTSIDE the `.brink-studio` root, where the scoped `position: fixed` styles and theme tokens never applied — the menu landed unstyled at the end of the document (the "right-click eats it / a scrollbar flashes" bug). `App` now accepts children inside the root and the popup hosts mount there.
