---
"@brink-lang/studio": minor
---

Adjustable text size, on both knobs. The **editor** has its own size
(Mod-= / Mod-- / Mod-0, the palette, or Settings), and the **app** has one
that scales the whole UI. Behind the app knob, 179 hardcoded font sizes
across 24 stylesheets were replaced by a nine-step type scale derived from
a single `--bs-font-base`, so components now reach for a named step
instead of inventing a number. The sweep is pixel-identical at the default
size apart from twelve declarations that snapped 0.5px to the nearest
step. Also defines `--bs-font-mono`, which every use site referenced but
nothing declared.
