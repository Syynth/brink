---
"@brink-lang/web": patch
---

Lambda annotation precedence + eager incompatibility diagnostic (issue #1994, closing #1932, RULED 2026-08-01). Observable through `@brink-lang/web` under `types = strict`, brink dialect only (lambda syntax is native-only):

- A lambda's own **written** parameter/return annotation (`|k: int|: int { … }`) now governs that slot's resulting type unconditionally, narrowing #1910's body-derived read-back to the **unannotated** case only. Previously, a wrong body derivation could silently override a correct written annotation with no diagnostic anywhere.
- A body-derived type that disagrees with the lambda's own written annotation is now reported as a new diagnostic, **`E174`**, raised eagerly at the lambda's own declaration — not deferred to wherever the lambda is later called. Unlike the gradual/advisory `E063`, `E174` is `Error`-severity by default and not `[lints]`-downgradable.
- A param the lambda's own body re-binds (`|t: int| { let t = "a"; … }`) is excluded from this precedence change (falls back to the pre-#1994 posture unconditionally) — the shadowing local's type is not the param's own narrowing.
- Unannotated params/returns are unaffected: #1910's fix (body-derived wins) is unchanged.

See `docs/typed-mode-spec.md` §2 for the full ruling, reconciled against the "annotation = firewall" wording alongside the existing top-level-`fn`/`flow` precedence rule.
