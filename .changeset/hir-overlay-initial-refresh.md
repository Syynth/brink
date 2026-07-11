---
"@brink-lang/editor": patch
---

HIR overlay now refreshes when the initial compile completes (#494). The
overlay's projection StateField seeded at view creation — before the first
async compile/analysis finished — and only recomputed on doc-changing
transactions, so a passively loaded file rendered no `brink-hir-*` marks or
rails until the first edit. `DocumentSessions` now dispatches a redecorate
effect to every mounted view whenever a compile result is delivered, and the
new `refreshHirOverlay(view)` / `refreshHirOverlayEffect` exports let hosts
with custom wiring re-read the projection from their own compile-complete
signal (mirrors `refreshGutterMarkers`).
