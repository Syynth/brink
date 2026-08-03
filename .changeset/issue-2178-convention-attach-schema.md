---
"@brink-lang/web": patch
---

Issue #2178 (split from #2164's 2026-08-03 design-backport comment, item 2):
`@[convention(…)]` gains an optional `attach = StructName` clause — the
handler's declared output **schema**.

- **`attach = StructName`** declares which keys a claiming handler attaches
  to the run it claims, by naming an ordinary declared `struct`: the schema
  is a type, not a new declarative sub-language. The governing split: keys
  are declared (this clause), values are computed (the handler body).
- The declaration's own `: Type` return-type annotation must name the same
  struct `attach` does, or the declaration is **E180** and is never
  registered as a claiming handler at all (the same "never a partial one"
  posture `E159`/`E178` already take).
- `@[element(args = "…")]` has no `attach` clause of its own — like `order`,
  this is `@[convention]`-only, since a self-announcing handler's output
  isn't a claim result to attach.

No runtime behavior changes for an existing `@[convention]` declaration that
does not use `attach` — this is purely additive.
