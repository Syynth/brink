---
"@brink-lang/studio": patch
---

The worker road is on by default (editor worker architecture W5 flip, decision log 2026-08-25): `mountStudio` defaults `workerSession` to true, so the project-level pulls — compile, outline, story graph, closure — run off the main thread in every studio and desktop embedding without opt-in. Fully feature-detected: environments without Web Workers (or where the worker fails to boot) silently keep the in-process road. Pass `workerSession: false` (or `?worker=0` in the playground) to force in-process.
