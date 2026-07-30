---
"@brink-lang/web": patch
---

#1728: `content::tag()`'s free-text scan (parsed by `brink-syntax-native`,
which `@brink-lang/web` pulls in transitively through `brink-compiler`) no
longer stops at the first literal `}` inside a `#tag` — a `}` that only
closes a `{…}` the tag's own raw text already echoed (an embedded
interpolation or alternation brace, e.g. `Hello #tag {gold} coins.`) no
longer fools the enclosing block's closer into ending early. Previously
this produced a spurious "unexpected token" parse error in both
`EditorSession` and `IdeSession`; that source now parses with zero errors.
An unbalanced `}` (including a legitimate enclosing-block closer with no
matching `{` inside the tag) still terminates the tag immediately, exactly
as before.
