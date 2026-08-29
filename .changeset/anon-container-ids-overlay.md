---
"@brink-lang/web": patch
---

HIR overlay spans for anonymous weave containers — choices, gather
continuations, conditional/sequence branches, inline sequences — now
carry the compiled program's real `DefinitionId` in `def_id` (previously
`null`; only named containers had identity). A labeled choice reports
the label's own id. This is the #3234 anonymous-container identity join:
the ids equal codegen's by construction, so debugger addresses inside
weaves can join back to source through the overlay.
