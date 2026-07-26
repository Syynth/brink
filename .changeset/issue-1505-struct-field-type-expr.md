---
"@brink-lang/web": patch
---

#1505: `brink-syntax-native`'s `struct_field` grammar widened from a bare
dotted `PATH` to the full `type_expr` production (function types, generic
instantiations). A `.brink` source compiled through `@brink-lang/web` may
now declare a function-typed field (`greet: fn(int): int`) or a
container-typed field (`list<int>`, `map<K, V>`) where it previously hit a
parse error. A `::`-qualified struct field type (`geo::Point`) is a new,
documented gap — `type_expr` accepts a single `IDENT` only, matching the
same restriction the brink dialect's own type-annotation grammar already
has everywhere else.
