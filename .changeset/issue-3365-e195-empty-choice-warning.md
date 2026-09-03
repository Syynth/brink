---
"@brink-lang/web": patch
---

New warning-level diagnostic `E195`: a `*`/`+` choice with no divert (even
an empty `* ->`), no tag directly on the line, and no text in any of its
three same-line content regions — matching inklecate's own "Choice is
completely empty" warning. A `(label)` or `{condition}` guard does not
exempt a choice either, matching the reference. Ink surface only; the
Problems panel now surfaces it wherever the pipeline previously stayed
silent on the shape. `Warning`-tier and `[lints]`-overridable, like its
`E164`/`E188`/`E193` neighbours.
