---
"@brink-lang/web": patch
---

Fix a compile failure (`E060` duplicate `DefinitionId`) when three or more inline conditionals share one content line: lifting nests the line's constructs, and cloned branch ids were re-derived from the host branch index alone, so two lift levels could produce the same id. The derivation now mixes the lifting construct's own identity into the salt (#3386).
