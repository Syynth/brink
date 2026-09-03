---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

A choice carries its kind and its source (#3435): `Choice.sticky` (`+`
vs `*`, as written) and `Choice.source` (the choice text's location, the
same shape a line's provenance uses) on both the journaled `choices`
line and the debug snapshot's `pending_choices`. The studio's transcript
echo of a taken choice (`> text`) now records `choiceKind` and `source`,
so the Player can draw the marker and link the echo back to the script.
