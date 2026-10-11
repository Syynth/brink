---
"@brink-lang/web": patch
---

A divert written on a choice line now runs before the line ends, in every
form ink allows there, as ink does (#3698). `+ hi ->->` delivers
`hi after` instead of ending the line at `hi`, and `+ hi -> t ->` keeps the
tunnel's first line on the choice's line. A multi-target divert on a choice
line (`+ hi -> a -> b`) now runs every target; brink used to drop all but
the first.
