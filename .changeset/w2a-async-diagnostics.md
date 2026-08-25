---
"@brink-lang/editor": patch
---

Diagnostics compile rides the async session facade (editor worker architecture W2a, `docs/editor-worker-spec.md`): `ProjectSession` owns a `SessionClient` (over the in-process `LocalTransport` for now) and gains `compileProjectAsync()` — same generation cache as the sync road, plus in-flight dedup so concurrent views share one compile. The diagnostics extension accepts a sync-or-async `compile` and lands async results under staleness guards (doc moved, view detached, plugin destroyed → the landing is discarded; a newer compile follows). `ProjectSession.destroy()` rejects in-flight client queries before freeing the wasm handle. Embedding hosts passing a synchronous `compile` are unaffected.
