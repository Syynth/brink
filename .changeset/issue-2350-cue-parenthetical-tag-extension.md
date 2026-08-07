---
"@brink-lang/web": patch
---

Prose dialect: a trailing `#tag` on a `CUE` or `PARENTHETICAL` line
(§8d.4, `@VENDOR #(v.o.)`) no longer declines the claim outright (issue
#2350).

`hir::lower_native::element::candidate` strips the tag before pattern
matching, exactly as issue #2077 already ruled for a scene heading's
`[slug]`/`#tag`s — one literalness doctrine across every claimed shape.
The stripped tag is recovered and delivered through the existing
`Content.tags` channel, the same interim carrier heading tags already use
pending #474's per-flow tag API. An `attach = StructName` claim (issue
#2178) still declines a tag-bearing cue/parenthetical outright, for the
same reason it already declines a tag-bearing heading: attach mode emits
no `Stmt::Content` at all, so there is no line for the recovered tag to
ride on.
