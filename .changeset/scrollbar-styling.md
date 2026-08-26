---
"@brink-lang/studio": patch
---

Scrollbars are styled to blend with the theme instead of using the loud
platform default: no track, a thin rounded thumb tinted from the theme's
muted foreground (so all five themes, light and dark, get a correct thumb),
darkening on hover and drag. Applies to every scrollable surface under the
studio root — the editor, tool windows, the binder, the search results —
and is overridable per theme via `--bs-scrollbar-thumb`,
`--bs-scrollbar-thumb-hover`, and `--bs-scrollbar-thumb-active`.
