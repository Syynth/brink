---
"@brink-lang/web": patch
---

Diagnostics now spell the option type `Option<T>`, the way the native type
grammar and the book do, instead of `Option[T]` (#2813). Changed: E116 ("an
`Option<T>` has no truthiness"), E147 and the `or`-coalescing message.
