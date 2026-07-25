---
"@brink-lang/web": patch
---

#1414: `EditorSessionHandle` gains `discoverProjectConfig(entry)`, the
web-mount counterpart of `brink compile`/`brink ide`'s `brink.toml`
discovery. Previously the wasm editor session had no discovery of its own —
`applyProjectConfig` only applied text an embedder had already located and
read through its own host filesystem API (Node `fs`, the File System Access
API, …), unlike every other mount, which resolves `brink.toml` by walking a
`SourceTree` (`brink_project_config::discover_from_entry_in_tree`).
`discoverProjectConfig` closes that gap for brink-web specifically: it
walks the session's own in-memory document tree (whatever `updateFile` has
loaded) up from `entry`'s directory, exactly like `brink compile`/
`brink ide` walk a real filesystem — no host-specific directory-walk code
required. Serve `brink.toml` as an ordinary document
(`updateFile("brink.toml", text)`) and call `discoverProjectConfig(entry)`
once; it applies `[project] dialect`/`types` and `[lints]`/`deny-warnings`
exactly like `applyProjectConfig` (same explicit-call precedence, same
warnings-array/re-analyze contract), and returns `[]` when no `brink.toml`
is found anywhere in the tree. `applyProjectConfig` is unchanged and stays
available for embedders that prefer handing text in directly.
