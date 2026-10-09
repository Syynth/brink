---
"@brink-lang/web": patch
---

`brink.toml` accepts a `[host]` table with one key, `manifest`: the path
(relative to `brink.toml`) of the host manifest JSON the CLI and the
language server load (#1784, #3666). The web studio reads the same config
parser, so a `[host]` table no longer reports as an unknown top-level key,
and a `[host]` that is not a table, or a `manifest` that is not a
non-empty string, is now a config error. The web studio itself does not
load the file yet: a manifest still reaches it only through
`setHostManifest` / `mountStudio({ hostManifest })`.
