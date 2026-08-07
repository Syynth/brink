---
"@brink-lang/web": patch
---

#2293: prose text no longer classifies as `variable`/`operator`/`string` in
semantic tokens, for both ink and native (`.brink`) files. This is the
remainder of the #2280/#2286 prose-classification gap — that PR fixed
declarations (`struct`, `speaker`, `convention`, ...) plus keyword lexemes
and `SCENE_TITLE` absorbed into prose, but left ordinary dialogue/narration
words, prose punctuation (`-`, `!`, `?`, `->`, `<>` reachable inside a raw
text/tag run), and a literal quote mark in dialogue still painting as code.

Fixed at the classifier level, not by suppressing output: a token whose
parent is a pure-prose CST node (`TEXT` for ink; `TEXT`/`TAG`/`SCENE_TITLE`
for native, `CUE_NAME` already handled) now classifies as no token at all,
matching the CST-presentation gap's own established precedent (`is_prose_
run_container`, #2286) and how `@codemirror`'s decoration model already
treats an unclassified range — plain default-foreground text, not a
missing/broken highlight. No new token type was introduced; the LSP
semantic-token legend is unchanged.
