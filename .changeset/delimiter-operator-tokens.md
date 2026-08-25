---
"@brink-lang/web": patch
---

Content-logic delimiters classify as operators: the `{`/`}` around inline alternatives, conditionals, and interpolations — and the `|` between alternative branches — now carry an operator semantic token instead of no token at all, so they render in the code color rather than blending into the surrounding dialogue/action prose (author feedback). Prose-absorbed and escaped braces/pipes stay uncolored, in both the ink and native classifiers.
