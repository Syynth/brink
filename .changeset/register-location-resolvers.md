---
"@brink-lang/studio": patch
---

The program and session location resolvers are now registered (W3/#3296):
a program-address Location resolves to source through the live session's
DebugInfo road, gated on `sessionDegraded` at the caller (suppressed
before the provider is even consulted — never stale), and a
position-shaped session ref chains session → program → source. The
symbol resolver moves into the same `registerLocationResolvers` module.
