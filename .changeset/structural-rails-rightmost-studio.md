---
"@brink-lang/studio": patch
---

Structural rails CSS (#3501, ruled 2026-09-03): the rails bar layer packs
with `gap: 0` and tighter side padding (2px -> 1px), so the reserved
one-lane column shrinks from 7px to 5px alongside `@brink-lang/editor`'s
`RAIL_LANE_WIDTH_PX`. The rail tooltip now renders a list of entries (one
`.brink-rail-tooltip-entry` per container in the line's stack, outermost
first) instead of a single label/meta pair.
