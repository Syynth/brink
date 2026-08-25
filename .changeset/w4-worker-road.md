---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

The session worker (editor worker architecture W4, `docs/editor-worker-spec.md` §8): `WorkerTransport` + a session worker entry running `SessionHostCore` — the exact host semantics `LocalTransport` runs, extracted and shared so the two transports cannot drift — with a boot handshake and crash fallback. `ProjectSession` gains `projectQuery`: the project-level pulls (compile, outline, story graph, closure) run in the worker's own wasm session, kept current by an ordered file/config mutation stream flushed before every worker query; `triggerCompile` (the last synchronous compile caller) rides the async facade. Opt-in via `MountStudioOptions.workerSession` (the playground's `?worker=1`); fully feature-detected — environments without workers, boot failures, and crashes all keep the in-process road. In worker mode the main thread records zero compile time: the whole-project compile (up to ~1.8 s cold on studio-scale projects) leaves the UI thread entirely.
