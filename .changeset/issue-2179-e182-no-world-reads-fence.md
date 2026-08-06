---
"@brink-lang/web": patch
---

Issue #2179: a `@[convention]` handler whose transitive call closure reaches a `Query`-kind (world-reading) or unclassified (`Plain`-kind) `EXTERNAL` now raises a new diagnostic, **`E182`**, anchored at the real offending call site. A `@[convention]` handler may call pure functions and `Effect`/`Presentation`-kind externals ("commands"), but must never read world state — classification has to stay a pure function of the text, since the editor, the projection cache, and explain-match all depend on it. This is web-observable: the wasm editor's diagnostics surface `E182` in its warnings JSON exactly like any other analyzer diagnostic.
