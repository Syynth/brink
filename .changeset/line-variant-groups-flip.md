---
"@brink-lang/web": patch
---

A content line whose inline stateful alternatives are all textual now
compiles them as SHARED alternatives — ink's documented semantics — instead
of cartesian clones with independent visit counts: `Line: {a|b} {x|y}`
produces `a x` / `b y` / `b y` across three views, where the second view
previously produced `b x` (#3271). The compiled artifact carries one
`LineVariantGroups` record per such line over whole-line variant entries
(each still its own translation unit and VO slot), and a labeled choice
inside an inline `{if …}` beside an alternative now compiles instead of
failing with an internal duplicate-container error (#3272). Lines whose
alternatives enumerate past 32 whole-line variants are refused with the new
worded E191.
