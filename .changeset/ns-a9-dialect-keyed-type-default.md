---
"@brink-lang/web": patch
---

Analyzer: the unset-`types` default is now dialect-keyed (NS-A9, ruled
2026-07-19) — a `"brink"`-dialect session with no explicit type policy
resolves `types = strict`; a `"strict-ink"` session resolves `gradual`
exactly as before. Resolution happens at one seam
(`brink_analyzer::resolve_type_policy`), and an explicit choice always
wins: `setTypePolicy(...)`, a `brink.toml` `types` key applied through
`applyProjectConfig`, or the CLI's `--types` all override the
dialect-keyed default.

Observable through `@brink-lang/web`: a brink-dialect editor/compile
session that never calls `setTypePolicy` now surfaces the strict-mode
diagnostics (`E065`/`E066`/`E067`, narrowed coercion lattice) that
previously required an explicit `setTypePolicy("strict")`. Opting out is
`setTypePolicy("gradual")` or `types = "gradual"` in `brink.toml`.

Also: `setTypePolicy` with an unrecognized value now behaves like never
calling it at all (the dialect-keyed default stays in effect) instead of
being treated as an explicit gradual opt-out — carrying the pre-NS-A9
"any other value keeps the default" contract forward, so garbage input
can never silently opt a brink session out of strict.

The oracle-anchored strict-ink surface is untouched by construction:
strict-ink + unset `types` resolves `gradual`, and strict-ink + explicit
`strict` remains the `E064` config error.
