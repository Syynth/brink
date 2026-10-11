---
"@brink-lang/web": patch
---

Argument completion offers an `enum` constraint's members. A host-manifest type with an `enum` constraint and no `values` source now completes its members, as string literals, in an argument slot.

A host manifest param's type may be written `type` as well as `ty`, the key a widget uses. Before, `type` was silently ignored and left the param untyped. A string type's value items written bare (`rain`) now complete as quoted literals.

An `EXTERNAL` declared with an ink fallback function of the same name keeps its host-manifest types in signature help, inlay hints, value labels, argument widgets and colour swatches. These used to pick the fallback, which has none.
