---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Performance instrumentation ships in all builds (prod-perf ruling 2026-08-25): the probe, browser observers, `__brinkPerf` harvesting global, and the Performance tool window are no longer dev-only — `mountStudio` enables them by default and `perf: false` (or the playground's `?perf=0`) strips the whole surface. The session worker now runs its own probe and wasm counters, reported through new host-level queries (`hostPerfReport` / `hostPerfReset` / `hostPerfSetEnabled` — answered by the hosting realm, never the session facade), and the HUD grows worker-plane and wasm-counter sections plus a combined JSON export; since W5 the analysis cost lives in the worker, so a main-thread-only panel could not see it. The probe's User Timing mirror now periodically clears its own entries (only its own — an embedding page's timeline is untouched), bounding an always-on session's growth. Perf payloads remain structurally content-free: static span/counter names and numbers only, nothing from the author's project.
