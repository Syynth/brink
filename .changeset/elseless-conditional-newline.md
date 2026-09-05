---
"@brink-lang/web": patch
---

A whole-line inline conditional with no else arm (`{cond:then}` alone on its
line) now keeps its line's newline when the condition itself prints. The
normalization lift gives the then-arm a line clone plus an end-of-line, but
synthesized an else arm only when there was prefix or suffix text to carry —
so with the construct alone on its line, the all-false path emitted no
end-of-line at all, and a printing condition's output ran into the next line
(`{f():a}` with a printing, false `f` gave `ab` where ink gives `a` / `b`).

ink keeps a line's `\n` whichever arm ran, suppressing it only when the line
produced no content. A silent `{false:a}` therefore still emits nothing: the
runtime drops a newline with no content before it.
