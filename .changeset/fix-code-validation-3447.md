---
"@brink-lang/web": patch
---

`brink.toml`'s `[fix]` table now validates its code keys against the real
diagnostic-code set, mirroring `[lints]`'s existing unrecognized-code
warning: an entry like `[fix]\nE9999 = "auto"` now reports a `ConfigWarning`
("`[fix]` `E9999` is not a recognized diagnostic code; ignored") instead of
being silently accepted and doing nothing. Surfaced through
`EditorSession::apply_project_config`'s returned warnings, the same channel
`[lints]` already used.
