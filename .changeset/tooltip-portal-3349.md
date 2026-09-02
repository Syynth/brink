---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Fix editor tooltips (hover cards, lint popups, autocomplete) rendering
clipped under the Player pane. CodeMirror mounted every tooltip inside the
editor's own `.cm-editor` element, so a sibling pane with its own stacking
context or `overflow` (the Player split, `z-index: 30`) could clip or paint
over it — a `position: fixed` tooltip escapes scroll clipping, not an
ancestor's stacking order or `overflow` box.

Tooltips now reparent (`tooltips({ parent })`) into a dedicated
`.brink-tooltip-layer` mount point the shell renders inside the real
`.brink-studio` theme root (found via the same `closest(".brink-studio")`
lookup `widget-popover.ts` already uses), so `--bs-*` design tokens keep
applying — a headless embed with no `.brink-studio` root, or a host that
doesn't render the layer, falls back to `document.body`, which still escapes
the clip. The layer is a dedicated mount rather than `.brink-studio` itself
because CM6's own tooltip container is `position: relative`, not
`fixed`/`absolute`; mounted directly on `.brink-studio` (a flex column) it
became an in-flow flex item that broke the shell's layout. `@brink-lang/studio`'s
tooltip CSS no longer requires a `.cm-editor` ancestor, since the reparented
node is no longer inside one.
