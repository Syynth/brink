---
"@brink-lang/editor": patch
---

Deferred-refresh consumers ride the async session facade (editor worker architecture W2b, `docs/editor-worker-spec.md`): the quiet-fire for refined highlight tokens, the HIR overlay/occurrences, inlay hints, argument widgets, and fold ranges now runs an async warm-up (`prepare*` options) through the `SessionClient` — background priority, per-surface coalesce keys — and dispatches the refresh effect only when it settles, under landing guards (doc moved or view destroyed → skipped; rejected warm-up → the field's synchronous fallback still refreshes, never stranding a view). Fields themselves are unchanged; small documents keep their synchronous rebuilds. Hosts not passing the new `prepare*` options get the previous behavior exactly.
