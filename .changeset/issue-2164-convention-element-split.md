---
"@brink-lang/web": patch
---

Issue #2164 (`docs/decision-log.md` 2026-08-03): `@[element(…)]`'s
pattern-claiming half splits into its own `@[convention(claims = "…",
order = N)]` annotation, and `order` becomes a required, bare-integer
precedence property.

- **`@[convention(claims = "…", order = N)]`** — pattern claiming: competes
  for prose lines it did not announce, confined to the `brink.toml`-named
  conventions module, and now REQUIRES `order` (no default — precedence is
  total, explicit, and authored, never inferred from declaration position).
  Two new diagnostics: **E178** (missing `order`) and **E179** (duplicate
  `order` within one module, reported against both declarations).
- **`@[element(args = "…", block)]`** — unchanged in meaning, narrowed to
  `!name` dispatch only: self-announcing, legal anywhere, no `order` at
  all (a self-announcing handler never competes for a line).
- The claiming walk's dispatch order is now `order`-sorted rather than
  declaration-order (the retired issue #1848 interim rule) — observable in
  which handler wins when two claiming patterns can both match one line.

Existing `@[element(claims = "…")]` source must be rewritten as
`@[convention(claims = "…", order = N)]`; `@[element(args = "…")]` is
unaffected.
