---
"@brink-lang/web": patch
---

New warning-level diagnostic `E195`: a `*`/`+` choice with no label, no
condition, no divert (even an empty `* ->`), and no text in any of its
three same-line content regions — matching inklecate's own "Choice is
completely empty" warning. Ink surface only; the Problems panel now
surfaces it wherever the pipeline previously stayed silent on the shape.
`Warning`-tier and `[lints]`-overridable, like its `E164`/`E188`/`E193`
neighbours.
