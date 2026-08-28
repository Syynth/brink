---
"@brink-lang/web": patch
---

Add the inverse debug resolver (#3246): `resolveSourceRange(file, start, end)`
on both `StorySessionHandle` and `StoryRunnerHandle`.

D9 mapped a running program address to source. This is the other direction —
the span of source text to the program address to break on — which is what a
breakpoint gutter needs, since the runtime keys breakpoints by
`(containerIdx, offset)` while an editor speaks in source.

Takes a half-open **byte** range rather than a line number: the runtime holds
no source text and no line table, so line-to-byte conversion belongs with the
caller, where the source already is.

Returns `null` when the span holds no executable code — a comment, a blank
line, a line whose code folded away — or when the artifact carries no debug
info. That `null` is a real answer callers must render, not an error to
swallow: refusing to arm a breakpoint visibly is better than arming one that
can never hit.
