---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

Drafts are editable in Settings, and each pattern shows what it matched

`[project] drafts` has been readable by the compiler since drafts landed and
editable nowhere — reaching it meant hand-editing `brink.toml`, and nothing
said whether the pattern worked. Settings ▸ General now lists the patterns
with add and remove, alongside the prose dictionary's shape.

Each row also reports what its pattern currently matches, because a bare list
of globs hides both of the ordinary mistakes. A pattern matching nothing — a
typo, or a renamed folder — looks exactly like one that is working, and now
says so. And a pattern matching a file the story still reaches produces no
draft at all (reachability wins), so those files are listed separately as
still in the story rather than silently counting for nothing.

`EditorSessionHandle.getDraftGlobReport()` exposes that per-pattern
attribution; draft status itself is still computed only in Rust.
