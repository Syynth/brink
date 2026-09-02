---
"@brink-lang/studio": patch
---

Settings' Diagnostics section gains a Fix column beside severity
(`docs/autofix-spec.md` §6.1): `off | ask | auto`, per diagnostic code,
written into `brink.toml`'s `[fix]` table through the same write path the
severity picker already uses for `[lints]`. `[fix]` and `[lints]` are
independent tables keyed by the same code, so a code's Fix policy shows and
edits regardless of whether it is also `[lints]`-configured.
