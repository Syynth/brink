---
"@brink-lang/web": patch
---

T2-4 effects tail (docs/effects-spec.md §10, issue #863): IDE hover now shows
a knot/stitch's inferred **effect row** on a stable line — `reads: …; writes:
…; calls: …`, or `pure`, or `opaque` for a definition that dispatches through
a function value. Purely advisory display; the only contract remains the
optional `#@effects` assertion (`E103` exceedance, unchanged).

Editor-observable through the shared `brink_ide::hover` path (LSP/wasm hover),
hence a `@brink-lang/web` patch. No behavior change to compiled output — effect
rows are additive metadata the runtime never reads.
