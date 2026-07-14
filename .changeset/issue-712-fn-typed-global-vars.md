---
"@brink-lang/web": patch
---

T1c follow-up (#712): a global `VAR`/`CONST` initialized with `#fn(...)`
(or annotated `fn(T…): R`) now carries its declaration-derived `Ty::Fn`
through to call-position checking under `types = strict`, instead of
escaping as `Unknown`. Observable through editor diagnostics:

- Calling directly through such a global (`heal_player(5)`, no local temp
  in between) type-checks against the target's known signature: arity/
  argument-type mismatches report `E063` exactly as they already did for a
  `#fn(...)`-initialized local temp.
- An explicit `VAR f: fn(int): int = …` annotation on the global now wins
  over inference, matching the existing annotation-wins firewall rule.
- Reassigning a fn-typed local from two globals with genuinely
  incompatible signatures still reports the pre-existing `E066`
  (Conflicted-escape) — previously masked because both globals silently
  escaped as `Unknown`, which unified without a conflict.
- Gradual mode is unaffected — these checks only ever run under
  `types = strict`.
