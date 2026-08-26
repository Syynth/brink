---
"@brink-lang/studio": minor
---

The Problems panel gains header controls: per-severity toggles (errors,
warnings, info/hints — each showing its count and muting that severity
when off), a funnel button that reveals a text filter over messages and
locations, and a group-by-file toggle with collapsible per-file sections
and per-file counts. The controls live in the panel's chrome header via
the new tool-window `actions` slot. Defaults reproduce the previous
panel exactly — every severity shown, ungrouped, no filter — so nothing
changes until a control is used.
