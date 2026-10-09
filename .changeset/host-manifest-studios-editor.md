---
"@brink-lang/editor": patch
---

`ProjectSession` follows the files `brink.toml` reads (#3671). After each
config apply it asks the provider for any it does not hold (`requestFile`),
tells it which to watch through a new optional `FileProvider.watchConfigFiles`,
and applies the config again when one of them changes — the host manifest
and `[dialogue]`'s file alike. Editing `dialect.json` used to leave
`[dialogue]` stale until `brink.toml` itself was touched. New getters:
`getConfiguredHostManifestError()` and `getConfiguredWarnings()`.
