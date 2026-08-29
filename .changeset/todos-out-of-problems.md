---
"@brink-lang/studio": patch
---

TODO notes get their own Problems-panel filter, **off by default**, so they
report in the TODOs panel without also filling Problems.

An author who wanted them out of Problems previously had only
`[lints] E189 = "allow"` to reach for. That suppresses the code at the
COMPILER, and the TODOs panel reads the same diagnostics — so turning them
off in one place emptied the other. Panel visibility is not a compiler
concern.

`E189` now buckets as `todo` rather than `info`, alongside the `prose`
bucket added earlier and for the same reason: it is a SOURCE, not a
severity, which is what lets it default off while `info` stays on. Turn the
bucket on to see TODO notes in both panels.

A stored preferences record written before this bucket existed reads as off,
so upgrading never puts TODO notes into Problems unasked.
