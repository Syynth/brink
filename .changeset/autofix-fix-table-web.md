---
"@brink-lang/web": patch
---

`brink.toml` now recognizes a `[fix]` table (`docs/autofix-spec.md` §6.1),
shaped like `[lints]`: `CODE = "off" | "ask" | "auto"`. `apply_project_config`
and `discover_project_config` no longer report `[fix]` as an unknown
top-level key, and an unrecognized value under it (`E033 = "sideways"`, or a
non-string/non-table shape) now surfaces as an error from those calls instead
of being silently ignored.
