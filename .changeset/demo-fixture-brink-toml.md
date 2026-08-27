---
"@brink-lang/studio": patch
---

The playground's default demo project now ships a `brink.toml`, so it looks
like a real project rather than relying on the host's constructor-time entry
argument — and the Settings view has something to show.

It declares `drafts = ["scratch/**"]`, and the demo gains
`scratch/cut-scene.ink`: deliberately not `INCLUDE`d, so the draft treatment
(#3145) is visible in the demo — Binder badge, draft mark beside the name,
and no "not included in the project" banner.
