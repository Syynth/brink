---
"@brink-lang/web": patch
---

Fix #1773: the E113 reserved-protocol-name walk (`display`/`compare`/
`next`, stdlib-spec §9.6) now descends into a lambda's own `|…|` params,
wherever the lambda literal sits — a VAR/CONST default, a temp initializer,
an assignment, a return value, a divert/tunnel/thread-start argument, a
content interpolation, a choice/if/while condition, or a native choice
label's `start_content`/`bracket_content`/`inner_content`. Previously a
lambda param named `display`, `compare`, or `next` was silently accepted
while an identically-named top-level fn/knot/stitch param was rejected.

This makes new hard E113 errors appear on `.brink` files that declare such
a lambda param, in both the studio Problems panel and through
`EditorSession`/`IdeSnapshot::analyze`.
