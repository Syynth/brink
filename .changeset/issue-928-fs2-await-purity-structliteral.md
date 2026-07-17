---
"@brink-lang/web": patch
---

FS-2 follow-up (#928, tracking #889): harden the `await`-condition purity
gate (E105) flagged in PR #935's review.

- The purity walk (`brink-analyzer::await_purity`) now recurses into
  `Expr::StructLiteral` field initializers in both the effectful-condition
  check and the salsa callee-collection path. An effectful call nested in a
  struct-construction condition (`await Flag#{on: raise_alarm()}`) previously
  slipped past E105 because `StructLiteral` was treated as a non-recursing
  leaf; it is now correctly rejected. (`FnLiteral` stays a leaf — a lambda
  body is not invoked during condition re-evaluation.)
- Added end-to-end coverage: a two-hop transitive write
  (`condition → outer() → inner() → writes a global`) trips E105, and an
  effectful call inside a struct-construction condition trips E105.

Wasm-observable: a program with such a condition, which previously produced
no E105, now surfaces the purity error through the diagnostics surface.
