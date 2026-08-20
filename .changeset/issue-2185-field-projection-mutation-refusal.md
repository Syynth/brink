---
"@brink-lang/web": patch
---

Mutating a record field projection (`pop(a.items)`, `heap_pop(a.items)`, `~ a.count++`, or passing `a.items` to a `ref` parameter) previously compiled clean and misrouted the mutation onto the whole record — faulting at runtime or, in the implicit-`ref` case, silently replacing the record's value. All four shapes now refuse at compile time under the existing non-suppressible E074 code, each with a message naming its actual shape (field-projection mutator argument, increment/decrement target, or implicit-`ref` argument — the last pointing at the supported explicit spelling, `f(ref a.items)`).
