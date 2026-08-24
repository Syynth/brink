---
"@brink-lang/web": minor
"@brink-lang/studio": minor
---

References dressing (card-stack PR E). New wasm entry point
`find_references_with_kinds_at` (wrapper:
`EditorSessionHandle.findReferencesWithKindsAt`): every reference site
classified by how it uses the symbol — `decl`, `call` (UFCS-desugared
calls included), `divert`, `read`, or `write` (assignment targets and
`++`/`--`). In the Search panel's references mode, the declaration card
pins first with an accent border and `decl` badge, and every site
carries its kind badge; the store re-resolves through the kinds variant
at the declaration anchor (plain locations remain the graceful
fallback).
