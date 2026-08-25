---
"@brink-lang/studio": patch
---

Panel pulls ride the async session facade (editor worker architecture W2d, `docs/editor-worker-spec.md`): the compile fan-out's three panel queries — project outline, compilation closure, story graph — run through the `SessionClient` at background priority with per-panel coalesce keys, and the store fan-out lands when they resolve, in the same relative order as before. A newer compile's fan-out supersedes an in-flight one wholesale (whole-project staleness class), and a dropped or failed pull keeps the last good panels.
