---
"@brink-lang/web": patch
---

B3a (#1482): UFCS resolution — `recv.name(args)` on the native `.brink`
surface is now resolved by a type-directed analyzer pass instead of failing
as an unresolved reference. A field on the receiver's type wins outright
(hard error `E140` when that field is not callable, never a silent
fall-through), otherwise the call desugars onto a free function in ordinary
lexical scope; neither is one diagnostic naming both attempts (`E141`), an
unknown receiver type demands an annotation (`E142`), and a `ref` first
parameter was refused here (`E143`); auto-ref lands separately in #1462, in
this same release. A resolved call is refused at lowering (`E144`) until the
verdict side table has a codegen consumer.

Web-observable through `compileProject`'s diagnostics: a `.brink` entry with
method-call syntax previously reported `E025` ("unresolved variable
reference") at every such site and now reports the specific ruled code, so
consumers filtering or grouping on `Diagnostic.code` see the new values.
Compiling `.ink` sources is completely unaffected — ink's own lowering
cannot produce the multi-segment callee path this pass keys on.
