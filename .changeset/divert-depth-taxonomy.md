---
"@brink-lang/editor": patch
---

Fix a headless-contract leak (#414, follow-up to #363): the line-decoration
pass stamped two inline `style` attributes regardless of `theme: false` —
`padding-left` for weave-depth indent on choices/gathers, and
`text-align: right` on standalone diverts — which beat host stylesheets and
left headless embedders unable to restyle them.

Both are now taxonomy instead:

- Weave depth rides as `data-depth="N"` (choices/gathers at depth > 1).
- Standalone diverts carry the `brink-divert-standalone` class.

`brinkTheme` ships the previous look (indent scaled by depth, right-aligned
standalone diverts) via CSS attribute/class selectors, so `brink-studio`
renders unchanged. Headless hosts (`theme: false`) restyle
`[data-depth="N"]` / `.brink-divert-standalone` directly — the
line-decoration pass never emits a `style` attribute.
