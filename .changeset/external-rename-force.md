---
"@brink-lang/web": patch
---

`EXTERNAL` functions are renameable behind the Force gate (ruled 2026-08-24): `prepare_rename`/`rename` accept them (declaration + every call site), but the safe-rename verdict is ALWAYS unsafe, carrying a new `E190` entry naming the host binding ("the engine must re-register the external under the new name") — so the rename only applies through the breakage report's Force path. Builtins remain non-renameable.
