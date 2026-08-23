---
"@brink-lang/studio": minor
---

Binder scope marks (#3014, #3021): the entry file carries an `entry`
badge; a source file outside the compile closure renders dimmed with a
`not included` badge (on disk, not in the story), with a legend
explaining both marks. The Library section (mounted stdlib) is hidden
entirely for ink projects, where the compiler provably excludes the
mounted `.brink` stdlib from every compile closure — it stays for
native entries and before the first compile.
