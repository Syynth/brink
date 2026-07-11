---
"@brink-lang/editor": patch
---

Fixed narrative-run folding in screenplay mode (#417). When a narrative run
IS a choice's body (a character cue + dialogue directly under `* [Talk]`),
the fold now anchors on the choice line itself and hides the whole body
beneath it, instead of anchoring one line down on the cue. The collapsed
pill no longer duplicates the anchor line's visible text ahead of the chip —
the fold now hides the whole anchor line and the pill IS the line, matching
the existing decl-fold placeholder shape. The pill's snippet also strips the
dialect's cue sigils and shows the first CONTENT line (or the cue's bare
name when the run has no content line), rather than raw text like
`@Jackie:<>`.
