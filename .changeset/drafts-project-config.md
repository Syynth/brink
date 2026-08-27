---
"@brink-lang/web": patch
"@brink-lang/studio": minor
---

`[project] drafts` in `brink.toml`: path globs naming work the author has
deliberately not wired into the story. A file matching one that is also
unreachable from the entry is a **draft** — it shows no "not included"
banner, and is marked as a draft wherever the studio names it (the Binder
row, the Continuous section heading, the Single File header, the Code
view's tab).

Reachability wins: a marked file the entry still `INCLUDE`s is not a draft
at all, so draft status can never exclude a file the story reaches.

New: `EditorSessionHandle.getDraftPaths()`, and a `documentMark` slot on
`ShellProvider` for any host that wants a status beside a document's name.
