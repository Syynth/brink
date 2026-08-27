---
"@brink-lang/web": patch
---

Escaped characters no longer receive semantic tokens. `\*`, `\[`, `\{` and
friends are prose by definition, but the escaped sigil's parent is an `ESCAPE`
node rather than `TEXT`, so it slipped past the classifier's prose carve-out and
`\*Party` painted its asterisk as an operator in the middle of a line of dialogue.
