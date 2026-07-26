---
"@brink-lang/web": patch
---

B4 display-boundary None-render (#1463, `docs/stdlib-spec.md` §1.6b):
an interpolation whose **final** value is `Option::None` now renders as
nothing instead of the interim total `"none"` (F28) — absence renders as
absence, the honest narrative meaning. This is cut by *position*, not by
type or dialect: nested compositions are never forgiven (`Option[T] ≠ T`
strictness holds everywhere else), and `string(none)` keeps rendering the
total `"none"` forever, unaffected. The forgiveness never loses
information — the append-only output transcript still records the raw
`Option::None` value, so a forgiven render is always traceable by
inspecting `OutputBuffer::transcript()`. Vanilla-ink stories are
byte-identical (`Option[T]` is a brink-dialect-only extension surface,
never reachable from ink-dialect source); the oracle corpus is
unaffected.
