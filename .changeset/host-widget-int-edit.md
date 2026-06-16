---
"@brink-lang/studio": patch
---

Fix host-widget Edit on non-string arguments. A host `ArgumentWidget` on a
non-string semantic type (e.g. an `int`) opened and called `host.resolve(...)`
but never wrote back when replacing an existing literal — the in-place edit
resolved the literal range with a quote-only finder, so a bare literal like `1`
was a silent no-op. Bare int/float/bool literals are now handled, so host
widgets can edit already-filled arguments of any type (#242).
