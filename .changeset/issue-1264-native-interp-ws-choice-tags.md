---
"@brink-lang/web": patch
---

`brink-syntax-native`: two content-family parser fixes (issue #1264).
Whitespace separating two interpolations on a content line (`{a} {b}`) is
no longer dropped — `content_items_until`'s significant-whitespace policy
now folds the pending trivia into its own `TEXT` node before a genuine
bare interpolation is parsed, so `"{a} {b}"` renders `"Alice Bob"` instead
of `"AliceBob"`. A choice line's trailing `#tag`s (`* Choice #tag1` inside
`{? }`) are now consumed by `choice()`'s own `tag_line_tail` call and
produce a real `TAG` node, instead of falling through to the enclosing
`choice_point` loop's `error_recover` and being wrapped in `ERROR` nodes.

`brink-web` transitively depends on `brink-syntax-native` via both
`brink-ir` and `brink-db` (non-optional): `brink-db::lowered_query`
dispatches `.brink`-extension files to `brink_syntax_native::parse` (the
#1106 seam), and `EditorSession::update_file` → `IdeSession::update_and_analyze`
passes the path through with no extension gate. Both fixes are therefore
wasm-observable for `.brink` files — most concretely the tag fix, which
changes the editor's diagnostics for `* Choice #tag` inside `{? }` from
ERROR-node parse errors to clean.
