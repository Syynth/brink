---
"@brink-lang/web": patch
---

The E025 add-import quick fix now places a file's first `IMPORT` below its `INCLUDE` block when the file has a `#@module` header, instead of between the header and the `INCLUDE`s (#3448). Only the edit's insertion offset changes; the imported names are the same.
