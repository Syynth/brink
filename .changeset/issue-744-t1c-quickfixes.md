---
"@brink-lang/web": patch
---

IDE: quick-fix affordances for the T1c creation-site diagnostics
(E079–E081) and the `call()`/`bind()` strict over-arity diagnostics
(issue #744).

`code_actions`/`resolve_code_action` now offer:

- **E081** (`#fn(target, args…)` over-binding): "remove extra argument(s)",
  trimming the bound-argument list back to the target's declared param
  count.
- **E080** (`#fn(target, args…)` unbound `ref` param): "bind ref
  argument(s)", appending the matching durable global `VAR`(s) — offered
  only when every unbound `ref` param through the target's last declared
  `ref` param has an unambiguous same-named `VAR` in scope, so the fix
  always leaves the call fully bound.
- **`call(f, args…)`/`bind(f, args…)` strict over-arity** (`E063`, issue
  #733's checker): "remove extra argument(s)", trimming the call's trailing
  args back to the count the callee's known type accepts.

`E079` (target is not a function definition) has no offered fix — no single
mechanical rewrite recovers the author's intent. Both modules are
ink-frontend-only (`#fn(...)` has no native-dialect spelling; the
`call`/`bind` fix is scoped to the ink frontend in this PR, with native
`.brink` sites tracked as a follow-up).
