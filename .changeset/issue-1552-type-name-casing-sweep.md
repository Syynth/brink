---
"@brink-lang/web": patch
---

Type-name conformance sweep (#1552): the annotation-surface generic
heads are renamed per the 2026-07-19 casing partition (module segments
snake_case, type names UpperCamel) — `array<T>` → `Array<T>`, `map<K,V>`
→ `Map<K,V>`, `list<L>` → `List<L>`, `handle<K>` → `Handle<K>`.
Primitives are unaffected and stay lowercase (`int`, `float`, `bool`,
`string`, `void`, `divert`).

`Option<T>` and `Weighted<T>` are now annotatable — previously
unspellable on the annotation surface, so a function returning `Option`
had no way to pin its return type against strict inference (#1168).

The old lowercase generic-head spellings (`array<T>`, `map<K,V>`,
`list<L>`, `handle<K>`) no longer resolve and are a hard `E061`
("unknown type") through the brink-dialect pipeline.
