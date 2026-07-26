---
"@brink-lang/web": patch
---

Issue #530: `signature`/`db.signature` stayed the decls-only `signature_query`
(`resolution_index_query` drops `Param`/`Temp` locals entirely — issue #517),
so it always returned `None` for a local `DefinitionId` — a silent "hover
shows nothing for locals" trap. Added `db.local_signature(file, def)`, a
per-file locals path that resolves a local's own TM-2 `: type` annotation
without merging the decls-only and full symbol indexes (per #531).

`brink-ide::hover` now wires it in (`inferred_local_type_str`), reachable
through `@brink-lang/web`'s `EditorSession` hover: a `Param`'s declared
annotation still wins over inference exactly as before, and a `~ temp x:
type = …` ascription — previously skipped straight to body inference even
when it disagreed with the declared type — now correctly wins too.
