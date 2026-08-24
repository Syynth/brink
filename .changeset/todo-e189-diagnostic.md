---
"@brink-lang/web": patch
---

ink `TODO:` author notes now surface as `E189` Info-severity diagnostics (issue #3050). Lowering previously dropped `AUTHOR_WARNING` nodes silently; each now emits one diagnostic whose message carries the note's text (`TODO: <text>`), visible through every diagnostics channel (`compile`, Problems). Info severity never gates a compile, and the code is `[lints]`-tierable like any other (`E189 = "allow"` hides TODOs).
