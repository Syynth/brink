---
"@brink-lang/web": patch
---

VAR/CONST/`~ temp` initializers and plain assignments are now type-checked
against declared type annotations under `types = strict` (#1877, the
remainder of #1864 left after PR #1875's direct-call-argument half):

- `VAR v: int = "hi"` and `CONST V: int = "hi"` now report `E063` instead
  of compiling with zero diagnostics — the TM-2 firewall previously let an
  explicit annotation silently *replace* the initializer's own inferred
  type rather than being checked against it.
- `~ temp t: int = "hi"` now reports `E063` — the ascription was recorded
  purely as an Unknown-escape fallback, never compared against its own
  initializer.
- A plain assignment (`~ v = "hi"`) against a target's already-known
  declared type now reports `E063` too: a global `VAR`/`CONST` target
  (never checked before — globals are never joined into the `Ty::Conflicted`
  lattice at all), and an annotated `~ temp` target whenever the
  disagreement wouldn't already be independently reported as `E066` via the
  existing Conflicted-escape join (no double-reporting). A `Param`
  assignment target is deliberately excluded from this new check — a param
  annotation is a signature-firewall slot `annotations::mismatches` (E063)
  already owns, and disagreements there are already reported through it.
- The global `VAR`/`CONST` check above compares against the declaration's
  full derived type, not only an explicit `: type` annotation: an
  **unannotated** `VAR v = 5` is checked too, since its declared type is
  read the same way as an annotated one (the initializer literal's own
  inferred type) — `VAR v = 5` followed by `~ v = "hi"` now reports `E063`.

This closes the gap `content`'s (#1846) "never coerces to or from string"
invariant still had at these positions after #1875 landed the direct-call
half. Strict-mode-only; `types = gradual` is unaffected and keeps
deferring to the existing runtime type-mismatch fault.
