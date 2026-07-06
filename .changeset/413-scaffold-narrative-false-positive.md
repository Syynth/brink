---
"@brink-lang/editor": patch
---

Fix a false-positive in the conditional-scaffold classification pass added
by #413: ordinary narrative containing inline logic that happens to
start or end with a brace (a standalone inline conditional used as
narrative content, e.g. `{visited: You were here before.}`, or narrative
ending in a value interpolation, e.g. `You have {gold}`) was incorrectly
swept into `Logic` classification (`brink-logic`) merely because the line
started with `{` or ended with `}`. Only a conditional/sequence block's
own genuine opening/closing brace (bare `{`/`}`, or `{` followed by a
switch expression ending in `:`) is scaffold now — inline logic embedded
in narrative keeps its narrative/dialogue classification.
