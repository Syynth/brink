---
"@brink-lang/web": patch
---

`brink.toml`'s `[project] elements` key — the pointer to the project's
conventions module (issue #1844) — is renamed to `[project] conventions`
(issue #2180). The key predates the 2026-08-03 split of `@[element]`
(`!name`-dispatched) from `@[convention]` (pattern-claiming) and, post-split,
named a module of the latter, not the former.

The old `elements` spelling is **not** hard-broken: it is still accepted as
a deprecated alias — parsed into the same value and behaving identically
downstream (including `E169`'s conventions-module confinement check) — but
now surfaces a `ConfigWarning` naming the rename. If a `brink.toml` sets
both `elements` and `conventions`, `conventions` wins and a second warning
names the conflict. This is user-visible through
`@brink-lang/web`'s wasm-exported `EditorSession::apply_project_config`,
which surfaces every `ConfigWarning` `parse_str_at` returns.
