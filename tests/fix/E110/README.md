# E110 — deprecated `#@effects(…)` tag rewrite (issue #3426)

`greet`'s effects assertion is spelled with the deprecated tag-channel
`#@effects(reads: mood)`. The `Safe` fix rewrites the whole tag line to the
`@[effects(reads(mood))]` annotation spelling — translating the argument
list from the legacy **colon** mini-grammar to the annotation's **paren-clause**
mini-grammar, not just copying the parenthesised text (the two grammars are
not the same; see `crates/internal/brink-ide/src/effects_tag_fix.rs`'s module
doc). Both sides declare the identical bound — verified by parsing both
through the real production grammars and comparing the resulting
`{pure, silent, total, reads, writes, calls}` shape — so the rewritten
annotation stays a real assertion, not a mangled one.
