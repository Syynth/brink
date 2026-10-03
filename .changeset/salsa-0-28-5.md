---
"@brink-lang/web": patch
---

Upgrade the analysis engine's incremental-computation library (salsa) from
0.27.2 to 0.28.5, fixing RUSTSEC-2026-0308: a use-after-free that could be
reached when stale interned values were reused across edits. Query results
are unchanged; where salsa 0.28 detects the unsound state, it now panics
instead of reading freed memory.
