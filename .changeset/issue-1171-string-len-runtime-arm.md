---
"@brink-lang/web": patch
---

Fix #1171: `len(s)` on a string faulted `NotIndexable("string")` at
runtime — `collection_len` handled `Array`/`Map`/`Range` but had no
`Value::String` arm. Added one returning the char count (Unicode scalar
values via `str::chars().count()`), matching the char-count semantics
`char_at`/`find` already use for string indexing elsewhere in the
runtime, and the stdlib verb table's `len(… | string): int`. Compile-time
inference was already correct; this closes the runtime gap.
