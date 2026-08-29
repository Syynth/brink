---
"@brink-lang/editor": patch
"@brink-lang/web": patch
---

A character cue now teaches the prose dictionary the spelling the prose
actually uses, and the cue line itself is no longer spell-checked.

An ink cue is written in caps (`@GRISWOLD:<>`) while the prose that mentions
the same character is not (`Griswold`), and dictionary matching is literal —
so seeding the cue's own spelling left every prose mention of the character
underlined.

Two halves, and neither works alone:

- Cue names are seeded in title case rather than as written. Seeding *both*
  spellings does not work: with `["GRISWOLD", "Griswold"]` in the dictionary
  the all-caps use is still reported, because Harper's proper-noun metadata
  drives a capitalization rule that fires regardless.
- Character-cue lines are excluded from prose ranges. A cue is the speaker's
  name, not prose — the same category as the knot and stitch names prose
  checking already excluded — but an ink cue line is an ordinary content span
  to the HIR projection, so it was being checked. With title-case seeding it
  would now be reported.

`griswold` in prose is still flagged, which is the point: it is a real
misspelling of a proper noun. Parentheticals and dialogue lines are still
checked — those are written prose.

`@brink-lang/editor` exports `withoutCueLines`, the second half, for hosts
composing prose ranges themselves.
