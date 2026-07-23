---
"@brink-lang/web": patch
---

Native parser: a bare `flags F =` (no members after `=`) is now a parse error, and `flags F = ()` is the explicit empty set (LIST parity, ruled 2026-07-22). Fixes the one silent zero-progress recovery path in the flags declaration. Observable through the web editor's diagnostics for `.brink` files.
