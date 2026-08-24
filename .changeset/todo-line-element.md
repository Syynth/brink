---
"@brink-lang/web": patch
---

`line_contexts` reports a new `todo` line element for ink `TODO:` author-note lines (#3050) — a trivia-facet classification like comments (the HIR never sees `AUTHOR_WARNING`), so the editor's line-class road marks TODO lines on the wasm path, not just the regex fallback.
