---
"@brink-lang/web": patch
---

#1384: `brink-project-config`'s `ConfigError` (malformed TOML syntax, or a
recognized `brink.toml` key holding an out-of-range value) now carries the
file's path on every variant (`Toml`/`NotATable`/`WrongType`/`InvalidValue`
join the existing `Io`), and a byte span where the `toml` crate provides
one (`Toml`, i.e. malformed syntax — `ConfigError::span()`). Continues
#1369, which threaded the discovered path into `LoadError::Config`/
`ConfigRead` but left `ConfigError` itself pathless.

**Observable through `@brink-lang/web`:** `EditorSession::discover_project_config`
now resolves `brink.toml` through the new path-carrying `parse_str_at`
(rather than the pathless `parse_str`), so a rejected `Result`'s error
message text changes shape slightly — it now comes from `ConfigError`'s own
`Display` (which names the file, and for malformed syntax, its line/column)
rather than the hand-rolled `"invalid brink.toml at {config_key}: {e}"`
wrapper this function used to build. Still always a rejected `Result`, never
a panic, for the same malformed-`brink.toml` inputs as before.
`EditorSession::apply_project_config` (the pathless entry point — an
embedder pushing raw TOML text it read through its own host API, with no
discovered location to give) is unchanged: it still calls the original
pathless `parse_str`, which now falls back to the bare `brink.toml` label
rather than an unlabeled error.
