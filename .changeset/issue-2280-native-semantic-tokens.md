---
"@brink-lang/web": patch
---

Editor: semantic tokens now classify native (`.brink`) syntax correctly
instead of reading as `variable` (issue #2280).

`EditorSession::semantic_tokens`/`semantic_tokens_doc` previously always ran
`brink_ide::semantic_tokens::classify_token` over the file's **ink**-parsed
CST, even for a native `.brink` file — `ProjectDb::parse` runs the ink
frontend regardless of extension, only `ProjectDb::parse_native` is
dialect-correct. Running ink's grammar over native source produced a garbled
tree (ink has no `struct`/`flow`/`@[...]` grammar), so `struct`, a struct's
own name, its field names, an annotation's name/argument names, and type
references all fell back to the generic `variable` colour, and a quoted
string containing a character class (`"...[A-Z]..."`) was shredded into
several differently-coloured fragments because ink's tokenizer treats `[`/
`]` as significant.

`brink_ide::semantic_tokens` gains a native-CST classifier
(`semantic_tokens_native`/`semantic_tokens_range_native`,
`classify_native_token`) that dispatches on `brink_syntax_native::SyntaxKind`
directly, and `EditorSession::semantic_tokens_impl` now checks
`IdeSession::is_native` and routes a native file through
`IdeSession::syntax_root_native` instead. `struct`/`flow`/`@[...]`
declarations, struct fields, and annotation names/args now get distinct
token types; a string literal's interior (including lexer-significant `[`/
`]` inside it) renders uniformly as `string`.
