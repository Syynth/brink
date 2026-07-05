---
"@brink-lang/editor": minor
---

Emit option identity on choice/body lines (#364): every `Choice` line and its `ChoiceBody` lines now carry `data-option-path` (the full lineage of zero-based option indices through the weave, e.g. `"0.2.1"` — nested weaves first-class) and `data-option` (the convenience innermost index) as CM6 line attributes alongside the existing element class. Gather lines close their level's groups, so the next option at that depth starts a new group at index 0; knot/stitch headers reset the weave. Hosts can render per-branch rails (e.g. colored `border-left` on body lines) from these attributes without re-deriving the weave. Also exports the pure `assignOptionPaths` post-pass and adds `optionPath` to `LineInfo`.
