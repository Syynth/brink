---
"@brink-lang/studio": patch
---

The knot/stitch rename prompt (`SymbolRenamePrompt`, issue #2511) now seeds its name field
synchronously at mount instead of from a `requestAnimationFrame` callback. Previously the field
mounted empty and was filled a frame later, so between mount and that frame it was visible,
enabled and editable but blank — and anything typed during that window was overwritten when the
frame ran. The field is uncontrolled and the confirm path reads `input.value`, so a clobbered
rename degraded to `name === currentName`: the prompt closed as if the user had accepted the
existing name, silently performing no rename at all. Typing into the prompt the instant it opens
now keeps what you typed.
