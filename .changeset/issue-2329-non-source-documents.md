---
"@brink-lang/web": patch
---

Fixed brink-db treating non-source documents (`brink.toml`, `.md`, `.json`,
`.ink.json`) as ink source (issue #2329, the general follow-up to
#2318/#2327). These files no longer lower through the ink frontend, join the
project symbol index, or contribute diagnostics — a project's own
`brink.toml`/README/oracle-regeneration JSON can no longer plant a bogus
symbol-index entry or a bogus diagnostic just by sharing a session with real
source files. Observable through `@brink-lang/web`'s symbol index/outline:
the index and diagnostics streams the wasm package re-exports now only ever
reflect real `.ink`/`.brink` source. The files stay in the session (config
discovery still reads `brink.toml`; the editor still opens `.md` files as
plain documents) — this is a classification fix, not deletion.

Also fixed `file_language`'s case-sensitive extension comparison (`.INK` was
classified inconsistently from `.ink` on a case-insensitive filesystem) —
extension matching (`file_language`, `is_source_file`, and
`has_recognized_source_extension`) is now case-insensitive throughout.
