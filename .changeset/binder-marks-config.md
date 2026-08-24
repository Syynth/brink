---
"@brink-lang/studio": minor
---

Binder v2, part 4 (#3041, #3042): diagnostics marks and the pinned
config row. File rows carry error/warning counts (a file sums its
diagnostics — the roll-up rule; a knot/stitch shows its own, computed
from diagnostic spans against the symbol's body; Info/Hint never mark),
and brink.toml leaves the file tree for a dedicated pinned row above the
binder foot — gear icon, monospace name, click opens it (where the form
view renders).
