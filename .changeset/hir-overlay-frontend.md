---
"@brink-lang/editor": minor
---

HIR structural overlay in the editor (#454 phases 3–5): a queryable projection
StateField renders `brink-hir-*` inline marks with `data-*` identity, per-line
rail attributes plus a concentric rails gutter (knot/stitch/choice/gather/
branch), and identity-keyed occurrence highlighting. New `getHirProjection`
option on `BrinkStudioOptions`; `hirSpansAt`/`hirIdentityAt` query helpers.
Default skin styles only unresolved refs, occurrences, and rails — hir marks
stay visually inert for host theming.
