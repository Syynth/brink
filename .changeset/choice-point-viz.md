---
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Choice-point visualization (W11/#3304, RULED). At a choice stop the
editor lights the whole choice point: every PRESENTED choice's line gets
the success band (the plural highlight seam's headline case), and
authored siblings not added to the block dim with the reason beside
them — "once-only · used" (derived from the new id-keyed visit counts)
or the line's own failing condition ("gold > 20 = false", enriched from
source; a by-elimination catch-all). No new runtime seam beyond two
additive snapshot fields: `DebugChoice.def_id` and
`DebugState.visit_ids` — both `DefinitionId`-keyed, string-equal to the
HIR overlay projection's `def_id` (#3234's identity join, now verified
end to end on the studio compile road, including that the path-keyed
visit list genuinely drops anonymous choice bodies). The editor's
highlight seam gains the `rejected` kind with a `note` chip; degraded
still suppresses everything.
