---
"@brink-lang/web": patch
---

Issue #1840 Q4 (`docs/decision-log.md` 2026-08-01 "Conventions comptime: the four blocking rulings (#1840)"): `register`'s effect row is now wired — every `register(...)` call is a write to the named conventions-registry cell (`DefinitionId::CONVENTIONS_REGISTRY_CELL`, spelled `conventions_registry` in a `writes(…)` clause), correcting an earlier framing that treated it as pure. Observable through `@brink-lang/web`: a `.brink` project declaring `@[effects(pure)] fn conventions() { register(x) }` now correctly fails to compile with `E103` naming `conventions_registry`, where it previously compiled clean; `@[effects(writes(conventions_registry))]` is the spelling that passes. An `await` condition that calls `register(...)` is likewise now rejected by the purity gate (`E105`), matching every other intrinsic write.
