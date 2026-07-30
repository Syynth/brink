---
"@brink-lang/web": patch
---

#1728: `content::tag()`'s free-text scan, in `brink-syntax-native` (the
*native* `.brink` frontend — `@brink-lang/web` pulls it in transitively
through `brink-db`, which its `EditorSession`/`IdeSession` use), no longer
stops at the first literal `}` inside a `#tag` — a `}` that only closes a
`{…}` the tag's own raw text already echoed (an embedded interpolation or
alternation brace, e.g. `Hello #tag {gold} coins.`) no longer fools the
enclosing block's closer into ending early. Previously this produced a
spurious "unexpected token" parse error; that source now parses with zero
errors. An unbalanced `}` (including a legitimate enclosing-block closer
with no matching `{` inside the tag) still terminates the tag immediately,
exactly as before.

This is scoped to `.brink` native-syntax files only — `brink-db`'s
`file_language` routes `.brink` paths to this native parser and every
other extension (including `.ink`) to `brink-syntax`, the separate ink
frontend this fix does not touch. An `.ink` project sees zero behavior
change from this release.
