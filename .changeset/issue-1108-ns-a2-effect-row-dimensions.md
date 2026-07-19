---
"@brink-lang/web": patch
---

NS-A2 (#1108): the effect-row extension wave — three new row dimensions
(`emits`, `tags`, `faults`; conservative-total, per-SCC inferred, bool v1)
and the `@[effects(…)]` assertion final form (args from
{pure, silent, total} plus the existing reads/writes/calls clauses,
exceedance-only). The rows themselves are additive metadata (a new
`EffectRows` section version carrying an extension-flags byte; episodes
byte-identical), but the assertion surface is compile-behavior observable
through `@brink-lang/web`: new annotation-line syntax `@[effects(…)]`
parses in the brink dialect, and new diagnostics ship — E108
(`silent` exceeded: the definition can produce content), E109 (`total`
exceeded: the definition can fault), E110 (warning: the `#@effects(…)`
tag spelling is deprecated — it keeps parsing as an alias), E111 (unknown
annotation name), E112 (annotation line outside the knot/stitch
leading-run placement). Vanilla-ink stories are unaffected; the oracle
corpus is byte-identical.
